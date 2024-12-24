//
// session_router/config.rs
//
// Only the config data and bridging code, no second “SessionRouter” struct
//

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::filters::ConvertProtoConfigError;

/// The Protobuf config if you support xDS. If you don't need it, remove it.
#[derive(prost::Message)]
pub struct SessionRouterConfigProto {
    #[prost(uint32, tag = "1")]
    pub handshake_bytes: u32,
    #[prost(string, tag = "2")]
    pub dynamo_table_name: String,
}

/// A small YAML/JSON config used for local (static) config.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SessionRouterConfig {
    #[serde(default = "default_handshake_bytes")]
    pub handshake_bytes: usize,

    #[serde(default = "default_table_name")]
    pub dynamo_table_name: String,
}

fn default_handshake_bytes() -> usize {
    4
}
fn default_table_name() -> String {
    "MyMatchmakingTable".into()
}

impl Default for SessionRouterConfig {
    fn default() -> Self {
        Self {
            handshake_bytes: default_handshake_bytes(),
            dynamo_table_name: default_table_name(),
        }
    }
}

// If you need bridging to/from Protobuf:
impl TryFrom<SessionRouterConfigProto> for SessionRouterConfig {
    type Error = ConvertProtoConfigError;

    fn try_from(proto: SessionRouterConfigProto) -> Result<Self, Self::Error> {
        Ok(Self {
            handshake_bytes: proto.handshake_bytes as usize,
            dynamo_table_name: proto.dynamo_table_name,
        })
    }
}

impl TryFrom<SessionRouterConfig> for SessionRouterConfigProto {
    type Error = ConvertProtoConfigError;

    fn try_from(config: SessionRouterConfig) -> Result<Self, Self::Error> {
        Ok(Self {
            handshake_bytes: config.handshake_bytes as u32,
            dynamo_table_name: config.dynamo_table_name,
        })
    }
}
