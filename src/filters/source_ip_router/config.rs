//! src/filters/source_ip_router/config.rs
//! 
//! Holds the `Config` struct for SourceIpRouter,
//! plus the `Route` and `Cidr` types.

use ipnetwork::IpNetwork;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::{net::IpAddr, str::FromStr};

/// The top-level static config for the SourceIpRouter filter.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Default)]
pub struct Config {
    /// A list of routes for matching source IPs.
    pub routes: Vec<Route>,
}

/// A single routing rule: if the source IP matches any of `sources`,
/// we route to `endpoint`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Route {
    /// One or more CIDR notations (e.g. `192.168.1.0/24`).
    pub sources: Vec<Cidr>,
    /// The endpoint (e.g. `127.0.0.1:6001`) to route to if matched.
    pub endpoint: String,
}

/// A CIDR type wrapping `IpNetwork`, with JSON serialization logic.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Cidr(
    #[serde(with = "ipnetwork_serde")]
    pub IpNetwork
);

// Derive a custom schemars schema that treats it as a string.
impl JsonSchema for Cidr {
    fn schema_name() -> String {
        "Cidr".to_owned()
    }

    fn json_schema(gen: &mut schemars::gen::SchemaGenerator) -> schemars::schema::Schema {
        <String>::json_schema(gen)
    }
}

impl Cidr {
    /// Returns true if `ip` is contained in this CIDR range.
    pub fn contains(&self, ip: IpAddr) -> bool {
        match (self.0, ip) {
            (IpNetwork::V4(v4net), IpAddr::V6(v6)) => {
                if let Some(mapped_v4) = v6.to_ipv4() {
                    v4net.contains(mapped_v4)
                } else {
                    false
                }
            }
            _ => self.0.contains(ip),
        }
    }
}

impl FromStr for Cidr {
    type Err = ipnetwork::IpNetworkError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self(s.parse()?))
    }
}

////////////////////////////////////////////////////////////////////////////////
// (De)serialization helpers for IpNetwork (to treat it as a string in JSON).
////////////////////////////////////////////////////////////////////////////////
mod ipnetwork_serde {
    use super::*;
    use serde::{Deserializer, Serializer};

    pub fn serialize<S>(net: &IpNetwork, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&net.to_string())
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<IpNetwork, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        s.parse().map_err(serde::de::Error::custom)
    }
}
