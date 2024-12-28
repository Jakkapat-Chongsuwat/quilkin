#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct SourceIpRouter {
    #[prost(message, repeated, tag = "1")]
    pub routes: ::prost::alloc::vec::Vec<source_ip_router::Route>,
}
/// Nested message and enum types in `SourceIpRouter`.
pub mod source_ip_router {
    #[allow(clippy::derive_partial_eq_without_eq)]
    #[derive(Clone, PartialEq, ::prost::Message)]
    pub struct Route {
        #[prost(string, repeated, tag = "1")]
        pub sources: ::prost::alloc::vec::Vec<::prost::alloc::string::String>,
        #[prost(string, tag = "2")]
        pub endpoint: ::prost::alloc::string::String,
    }
}
