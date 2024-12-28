//
// session_router/mod.rs
//
// This file holds the *real* SessionRouter filter
//

use std::collections::HashMap;
use std::sync::Arc;

use aws_sdk_dynamodb::Client as DynamoClient;
use eyre::WrapErr;
use parking_lot::Mutex;
use tokio::task::JoinHandle;
use tracing::{debug, info, warn};

use base64::engine::general_purpose::STANDARD;
use base64::engine::Engine;

use crate::filters::prelude::*;
use crate::filters::{
    CreationError, DynFilterFactory, FilterFactory, FilterInstance, FilterKind, StaticFilter,
};
use crate::net::endpoint::{Endpoint, EndpointAddress};

// Just the config (no second SessionRouter in here)
mod config;
pub use config::{SessionRouterConfig, SessionRouterConfigProto};

/// We provide a `factory(...)` function that returns a [`DynFilterFactory`].
/// This is how Quilkin can build your filter from config.
pub fn factory(dynamo_client: Option<DynamoClient>) -> DynFilterFactory {
    Box::new(SessionRouterFactory { dynamo_client })
}

/// The factory object that implements [`FilterFactory`], hooking up config -> filter.
struct SessionRouterFactory {
    dynamo_client: Option<DynamoClient>,
}

impl FilterFactory for SessionRouterFactory {
    fn name(&self) -> &'static str {
        "quilkin.filters.session_router.v1alpha1.SessionRouter"
    }

    fn config_schema(&self) -> schemars::schema::RootSchema {
        // JSON schema for `SessionRouterConfig`
        schemars::schema_for!(SessionRouterConfig)
    }

    fn create_filter(&self, args: CreateFilterArgs) -> Result<FilterInstance, CreationError> {
        // 1) Convert Quilkin's `ConfigType` to JSON
        let config_val = match args.config {
            Some(crate::config::ConfigType::Static(json_val)) => json_val,
            Some(crate::config::ConfigType::Dynamic(_any_msg)) => {
                return Err(CreationError::NotFound(
                    "xDS dynamic config not implemented for SessionRouter".into(),
                ));
            }
            None => serde_json::Value::Null,
        };

        // 2) Parse that JSON => `SessionRouterConfig`
        let config: SessionRouterConfig = serde_json::from_value(config_val.clone())
            .map_err(|e| CreationError::DeserializeFailed(e.to_string()))?;

        // 3) Need a real Dynamo client
        let dynamo = self.dynamo_client.clone().ok_or_else(|| {
            CreationError::NotFound("No DynamoClient provided to SessionRouterFactory".to_string())
        })?;

        // 4) Build the actual SessionRouter
        let router = SessionRouter::new(config, dynamo);

        // 5) Put it into FilterKind's `SessionRouter` variant
        let filter_kind = FilterKind::SessionRouter(router);

        // 6) Wrap in FilterInstance
        Ok(FilterInstance::new(config_val, filter_kind))
    }

    fn encode_config_to_protobuf(
        &self,
        _config: serde_json::Value,
    ) -> Result<prost_types::Any, CreationError> {
        // If needed, implement bridging to Protobuf
        Ok(prost_types::Any::default())
    }

    fn encode_config_to_json(
        &self,
        _pb: prost_types::Any,
    ) -> Result<serde_json::Value, CreationError> {
        // If needed, implement bridging from Protobuf
        Ok(serde_json::Value::Null)
    }
}

/// The actual filter
#[derive(Debug)]
pub struct SessionRouter {
    config: SessionRouterConfig,
    sessions: Mutex<HashMap<EndpointAddress, Endpoint>>,
    token_map: Arc<Mutex<HashMap<String, String>>>,
    #[allow(dead_code)]
    poll_handle: JoinHandle<()>,
}

impl SessionRouter {
    pub fn new(cfg: SessionRouterConfig, dynamo: DynamoClient) -> Self {
        let token_map = Arc::new(Mutex::new(HashMap::new()));
        let poll_interval = std::time::Duration::from_secs(10);
        let table_name = cfg.dynamo_table_name.clone();

        let tm = Arc::clone(&token_map);
        let handle = tokio::spawn(async move {
            loop {
                match fetch_tokens_from_dynamo(&dynamo, &table_name).await {
                    Ok(latest) => {
                        *tm.lock() = latest;
                    }
                    Err(e) => {
                        warn!("Failed to fetch token map: {e}");
                    }
                }
                tokio::time::sleep(poll_interval).await;
            }
        });

        Self {
            config: cfg,
            sessions: Mutex::new(HashMap::new()),
            token_map,
            poll_handle: handle,
        }
    }

    fn lookup_endpoint(&self, token: &str) -> Option<Endpoint> {
        let locked_map = self.token_map.lock();
        locked_map
            .get(token)
            .and_then(|ip_port| ip_port.parse().ok())
            .map(Endpoint::new)
    }
}

/// Normal Filter trait
impl Filter for SessionRouter {
    fn read(&self, ctx: &mut ReadContext) -> Result<(), FilterError> {
        let src = &ctx.source;

        // check if we have a session
        if let Some(ep) = self.sessions.lock().get(&src) {
            debug!(?src, endpoint=?ep.address, "Reusing session");
            ctx.destinations.clear();
            ctx.destinations.push(ep.address.clone());
            return Ok(());
        }

        // need handshake
        let needed = self.config.handshake_bytes;
        if ctx.contents.len() < needed {
            warn!(?src, "Not enough handshake => drop");
            ctx.destinations.clear();
            return Ok(()); // effectively drop
        }

        let handshake_data = ctx.contents.split_prefix(needed);
        let token = STANDARD.encode(handshake_data);
        info!(?src, ?token, "Captured handshake token");

        // lookup
        if let Some(endpoint) = self.lookup_endpoint(&token) {
            info!(?src, endpoint=?endpoint.address, "Session established");
            self.sessions.lock().insert(src.clone(), endpoint.clone());
            ctx.destinations.clear();
            ctx.destinations.push(endpoint.address.clone());
        } else {
            warn!(?token, "No route => drop");
            ctx.destinations.clear();
        }

        Ok(())
    }

    fn write(&self, _ctx: &mut WriteContext) -> Result<(), FilterError> {
        // usually do nothing
        Ok(())
    }
}

/// For xDS usage, we still define the StaticFilter impl, but we’ll not
/// actually create it that way. We rely on the factory above.
impl StaticFilter for SessionRouter {
    const NAME: &'static str = "quilkin.filters.session_router.v1alpha1.SessionRouter";
    type Configuration = SessionRouterConfig;
    type BinaryConfiguration = SessionRouterConfigProto;

    fn try_from_config(_cfg: Option<Self::Configuration>) -> Result<Self, CreationError> {
        // Typically we bail out since we use the factory approach
        Err(CreationError::MissingConfig(Self::NAME))
    }
}

/// Helper that queries Dynamo => builds (token -> “ip:port”) map.
async fn fetch_tokens_from_dynamo(
    dynamo: &DynamoClient,
    table_name: &str,
) -> eyre::Result<HashMap<String, String>> {
    let mut map = HashMap::new();

    let resp = dynamo
        .scan()
        .table_name(table_name)
        .send()
        .await
        .wrap_err("Dynamo scan failed")?;

    // In newer AWS SDKs, `resp.items()` returns `&[HashMap<String, AttributeValue>]`
    for item in resp.items() {
        // Convert &String -> &str to match `unwrap_or("some_literal")`
        let token_str = item
            .get("token")
            .and_then(|v| v.as_s().ok())
            .map(|s| s.as_str())
            .unwrap_or("<NO_TOKEN>");

        let ip_str = item
            .get("ipAddress")
            .and_then(|v| v.as_s().ok())
            .map(|s| s.as_str())
            .unwrap_or("127.0.0.1");

        let port_str = item
            .get("port")
            .and_then(|v| v.as_n().ok())
            .map(|s| s.as_str())
            .unwrap_or("7777");

        map.insert(token_str.to_string(), format!("{ip_str}:{port_str}"));
    }

    Ok(map)
}

// tests
#[cfg(test)]
mod tests {
    use super::*;
    use crate::filters::ReadContext;
    use crate::net::endpoint::address::EndpointAddress;
    use crate::pool::BufferPool;
    use std::sync::Arc;

    #[test]
    fn test_session_router_basic() {
        let dummy = tokio::spawn(async {});
        let cfg = SessionRouterConfig {
            handshake_bytes: 4,
            ..Default::default()
        };

        // No real Dynamo
        let router = SessionRouter {
            config: cfg,
            sessions: Mutex::new(HashMap::new()),
            token_map: Arc::new(Mutex::new({
                let mut hm = HashMap::new();
                hm.insert("cm9vbQ==".into(), "10.0.101.69:7777".into());
                hm
            })),
            poll_handle: dummy,
        };

        let pool = Arc::new(BufferPool::new(1, 64));
        let mut buf = pool.alloc();
        buf.extend_from_slice(b"roomHELLO");

        let mut ctx = ReadContext::new_default(buf);
        ctx.source = EndpointAddress::from_string("127.0.0.1:5555").unwrap();

        let res = router.read(&mut ctx);
        assert!(res.is_ok());
        assert_eq!(ctx.destinations.len(), 1);
        assert_eq!("10.0.101.69:7777", ctx.destinations[0].to_string());
    }
}
