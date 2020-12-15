/*
 * Copyright 2020 Google LLC All Rights Reserved.
 *
 * Licensed under the Apache License, Version 2.0 (the "License");
 * you may not use this file except in compliance with the License.
 * You may obtain a copy of the License at
 *
 *     http://www.apache.org/licenses/LICENSE-2.0
 *
 * Unless required by applicable law or agreed to in writing, software
 * distributed under the License is distributed on an "AS IS" BASIS,
 * WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
 * See the License for the specific language governing permissions and
 * limitations under the License.
 */

use std::sync::atomic::{AtomicUsize, Ordering};

use rand::{thread_rng, Rng};
use serde::{Deserialize, Serialize};

use crate::config::UpstreamEndpoints;
use crate::extensions::{
    CreateFilterArgs, DownstreamContext, DownstreamResponse, Error, Filter, FilterFactory,
};

/// Policy represents how a [`LoadBalancerFilter`] distributes
/// packets across endpoints.
#[derive(Debug, Deserialize, Serialize, Eq, PartialEq)]
pub enum Policy {
    /// Send packets to endpoints in turns.
    #[serde(rename = "ROUND_ROBIN")]
    RoundRobin,
    /// Send packets to endpoints chosen at random.
    #[serde(rename = "RANDOM")]
    Random,
}

impl Default for Policy {
    fn default() -> Self {
        Policy::RoundRobin
    }
}

/// Config represents configuration for a [`LoadBalancerFilter`].
#[derive(Serialize, Deserialize, Debug)]
struct Config {
    #[serde(default)]
    policy: Policy,
}

/// EndpointChooser chooses from a set of endpoints that a proxy is connected to.
trait EndpointChooser: Send + Sync {
    /// choose_endpoints asks for the next endpoint(s) to use.
    fn choose_endpoints(&self, endpoints: &mut UpstreamEndpoints);
}

/// RoundRobinEndpointChooser chooses endpoints in round-robin order.
pub struct RoundRobinEndpointChooser {
    next_endpoint: AtomicUsize,
}

impl RoundRobinEndpointChooser {
    fn new() -> Self {
        RoundRobinEndpointChooser {
            next_endpoint: AtomicUsize::new(0),
        }
    }
}

impl EndpointChooser for RoundRobinEndpointChooser {
    fn choose_endpoints(&self, endpoints: &mut UpstreamEndpoints) {
        let count = self.next_endpoint.fetch_add(1, Ordering::Relaxed);
        // Note: Unwrap is safe here because the index is guaranteed to be in range.
        let num_endpoints = endpoints.size();
        endpoints.keep(count % num_endpoints)
            .expect("BUG: unwrap should have been safe because index into endpoints list should be in range");
    }
}

/// RandomEndpointChooser chooses endpoints in random order.
pub struct RandomEndpointChooser;

impl EndpointChooser for RandomEndpointChooser {
    fn choose_endpoints(&self, endpoints: &mut UpstreamEndpoints) {
        // Note: Unwrap is safe here because the index is guaranteed to be in range.
        let idx = (&mut thread_rng()).gen_range(0, endpoints.size());
        endpoints.keep(idx)
            .expect("BUG: unwrap should have been safe because index into endpoints list should be in range");
    }
}

/// Creates instances of LoadBalancerFilter.
#[derive(Default)]
pub struct LoadBalancerFilterFactory;

/// LoadBalancerFilter load balances packets over the upstream endpoints.
struct LoadBalancerFilter {
    endpoint_chooser: Box<dyn EndpointChooser>,
}

impl FilterFactory for LoadBalancerFilterFactory {
    fn name(&self) -> String {
        "quilkin.extensions.filters.load_balancer.v1alpha1.LoadBalancer".into()
    }

    fn create_filter(&self, args: CreateFilterArgs) -> Result<Box<dyn Filter>, Error> {
        let config: Config = serde_yaml::to_string(&args.config)
            .and_then(|raw_config| serde_yaml::from_str(raw_config.as_str()))
            .map_err(|err| Error::DeserializeFailed(err.to_string()))?;

        let endpoint_chooser: Box<dyn EndpointChooser> = match config.policy {
            Policy::RoundRobin => Box::new(RoundRobinEndpointChooser::new()),
            Policy::Random => Box::new(RandomEndpointChooser),
        };

        Ok(Box::new(LoadBalancerFilter { endpoint_chooser }))
    }
}

impl Filter for LoadBalancerFilter {
    fn on_downstream_receive(&self, mut ctx: DownstreamContext) -> Option<DownstreamResponse> {
        self.endpoint_chooser.choose_endpoints(&mut ctx.endpoints);
        Some(ctx.into())
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;
    use std::net::SocketAddr;

    use crate::config::{EndPoint, Endpoints};
    use crate::extensions::filter_registry::DownstreamContext;
    use crate::extensions::filters::load_balancer::LoadBalancerFilterFactory;
    use crate::extensions::{CreateFilterArgs, Filter, FilterFactory};

    fn create_filter(config: &str) -> Box<dyn Filter> {
        let factory = LoadBalancerFilterFactory;
        factory
            .create_filter(CreateFilterArgs::new(Some(
                &serde_yaml::from_str(config).unwrap(),
            )))
            .unwrap()
    }

    fn get_response_addresses(
        filter: &dyn Filter,
        input_addresses: &[SocketAddr],
    ) -> Vec<SocketAddr> {
        filter
            .on_downstream_receive(DownstreamContext::new(
                Endpoints::new(
                    input_addresses
                        .iter()
                        .map(|addr| EndPoint::new("".into(), *addr, vec![]))
                        .collect(),
                )
                .unwrap()
                .into(),
                "127.0.0.1:8080".parse().unwrap(),
                vec![],
            ))
            .unwrap()
            .endpoints
            .iter()
            .map(|ep| ep.address)
            .collect::<Vec<_>>()
    }

    #[test]
    fn round_robin_load_balancer_policy() {
        let addresses = vec![
            "127.0.0.1:8080".parse().unwrap(),
            "127.0.0.2:8080".parse().unwrap(),
            "127.0.0.3:8080".parse().unwrap(),
        ];

        let yaml = "
policy: ROUND_ROBIN
";
        let filter = create_filter(yaml);

        // Check that we repeat the same addresses in sequence forever.
        let expected_sequence = addresses.iter().map(|addr| vec![*addr]).collect::<Vec<_>>();

        for _ in 0..10 {
            assert_eq!(
                expected_sequence,
                (0..addresses.len())
                    .map(|_| get_response_addresses(filter.as_ref(), &addresses))
                    .collect::<Vec<_>>()
            );
        }
    }

    #[test]
    fn random_load_balancer_policy() {
        let addresses = vec![
            "127.0.0.1:8080".parse().unwrap(),
            "127.0.0.2:8080".parse().unwrap(),
            "127.0.0.3:8080".parse().unwrap(),
        ];

        let yaml = "
policy: RANDOM
";
        let filter = create_filter(yaml);

        // Run a few selection rounds through the addresses.
        let mut result_sequences = vec![];
        for _ in 0..10 {
            let sequence = (0..addresses.len())
                .map(|_| get_response_addresses(filter.as_ref(), &addresses))
                .collect::<Vec<_>>();
            result_sequences.push(sequence);
        }

        // Check that every address was chosen at least once.
        assert_eq!(
            addresses.into_iter().collect::<HashSet<_>>(),
            result_sequences
                .clone()
                .into_iter()
                .flatten()
                .flatten()
                .collect::<HashSet<_>>(),
        );

        // Check that there is at least one different sequence of addresses.
        assert!(
            &result_sequences[1..]
                .iter()
                .any(|seq| seq != &result_sequences[0]),
            "the same sequence of addresses were chosen for random load balancer"
        );
    }
}