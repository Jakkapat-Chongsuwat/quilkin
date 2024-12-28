//! src/filters/source_ip_router.rs
//! 
//! A custom Quilkin filter named "SourceIpRouter" that inspects
//! the client source IP and rewrites `ctx.destinations`.

mod config;

use crate::filters::prelude::*;
use crate::filters::error::ConvertProtoConfigError;
use crate::net::endpoint::address::EndpointAddress; // for ctx.destinations
use crate::filters::CreationError;
use tracing::debug;

use std::net::SocketAddr;

// Import our auto-generated Protobuf module, e.g.
// `crates/quilkin-proto/src/generated/quilkin/filters/source_ip_router/v1alpha1/source_ip_router.rs`
use crate::generated::quilkin::filters::source_ip_router::v1alpha1 as proto;

pub use config::{Cidr, Config, Route};

////////////////////////////////////////////////////////////////////////////////
// 1) Conversions between Rust `Config` and `proto::SourceIpRouter`
////////////////////////////////////////////////////////////////////////////////

impl From<Config> for proto::SourceIpRouter {
    fn from(cfg: Config) -> Self {
        proto::SourceIpRouter {
            routes: cfg
                .routes
                .into_iter()
                .map(|r| proto::source_ip_router::Route {
                    sources: r
                        .sources
                        .into_iter()
                        .map(|cidr| cidr.0.to_string())
                        .collect(),
                    endpoint: r.endpoint,
                })
                .collect(),
        }
    }
}

impl TryFrom<proto::SourceIpRouter> for Config {
    type Error = ConvertProtoConfigError;

    fn try_from(pb: proto::SourceIpRouter) -> Result<Self, Self::Error> {
        let mut routes = Vec::new();

        for r in pb.routes.into_iter() {
            // Convert strings -> Cidr
            let mut cidrs = Vec::new();
            for s in r.sources {
                let parsed = s
                    .parse()
                    .map_err(|err| ConvertProtoConfigError::new(
                        format!("Invalid CIDR '{s}': {err}"),
                        Some("routes.sources".to_string()),
                    ))?;
                cidrs.push(Cidr(parsed));
            }

            routes.push(Route {
                sources: cidrs,
                endpoint: r.endpoint,
            });
        }

        Ok(Config { routes })
    }
}

////////////////////////////////////////////////////////////////////////////////
// 2) The SourceIpRouter filter itself
////////////////////////////////////////////////////////////////////////////////

/// Filter that inspects `ctx.source` IP. If it matches any route,
/// we rewrite `ctx.destinations` to a single endpoint from that route.
pub struct SourceIpRouter {
    routes: Vec<Route>,
}

impl SourceIpRouter {
    fn new(cfg: Config) -> Self {
        Self { routes: cfg.routes }
    }
}

impl StaticFilter for SourceIpRouter {
    /// Must match the name in your YAML. E.g.:
    /// - name: quilkin.filters.source_ip_router.v1alpha1.SourceIpRouter
    const NAME: &'static str = "quilkin.filters.source_ip_router.v1alpha1.SourceIpRouter";

    // This must be `JsonSchema + Deserialize + Serialize + TryFrom<BinaryConfiguration>`.
    type Configuration = Config;

    // This must be `prost::Message + Default + TryFrom<Configuration>`.
    // Our Protobuf type from the generated code:
    type BinaryConfiguration = proto::SourceIpRouter;

    fn try_from_config(config: Option<Self::Configuration>) -> Result<Self, CreationError> {
        let cfg = Self::ensure_config_exists(config)?;
        Ok(Self::new(cfg))
    }
}

impl Filter for SourceIpRouter {
    fn read(&self, ctx: &mut ReadContext) -> Result<(), FilterError> {
        // convert EndpointAddress => SocketAddr => IpAddr
        let src_ip = ctx.source.to_socket_addr()?.ip();

        for route in &self.routes {
            if route.sources.iter().any(|cidr| cidr.contains(src_ip)) {
                debug!(
                    "SourceIpRouter matched route: source={} => endpoint={}",
                    ctx.source, route.endpoint
                );

                // parse route.endpoint => SocketAddr
               let socket_addr: SocketAddr = route.endpoint.parse().map_err(|_err| {
                    // Return a fixed, static error message
                    FilterError::Custom("Invalid endpoint address")
                })?;


                // Clear existing destinations, then push a single address
                ctx.destinations.clear();
                ctx.destinations.push(EndpointAddress::from(socket_addr));

                // stop searching, first match wins
                return Ok(());
            }
        }

        debug!("SourceIpRouter found no match for source={}", ctx.source);
        Ok(())
    }

    fn write(&self, _ctx: &mut WriteContext) -> Result<(), FilterError> {
        // Typically do nothing on the server->client path
        Ok(())
    }
}
