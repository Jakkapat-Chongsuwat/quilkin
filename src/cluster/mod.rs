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

use serde_json::value::Value;
use std::collections::HashMap;
use std::net::SocketAddr;

#[cfg(not(doctest))]
pub(crate) mod cluster_manager;

// Stub module to work-around not including cluster_manager in doc tests.
#[cfg(doctest)]
pub(crate) mod cluster_manager {
    pub struct ClusterManager;
    pub struct SharedClusterManager;
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Endpoint {
    pub address: SocketAddr,
    pub metadata: Option<Value>,
}

#[derive(Clone, Debug, Hash, Eq, PartialEq)]
pub struct Locality {
    pub region: String,
    pub zone: String,
    pub sub_zone: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LocalityEndpoints {
    pub endpoints: Vec<Endpoint>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Cluster {
    pub localities: ClusterLocalities,
}

pub type ClusterLocalities = HashMap<Option<Locality>, LocalityEndpoints>;