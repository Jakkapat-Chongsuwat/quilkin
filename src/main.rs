//
// src/main.rs
//

use std::sync::atomic::{AtomicUsize, Ordering as AtomicOrdering};

use clap::Parser;
use stable_eyre; // For better error reporting
use tracing::{error, info};

use aws_config;
use aws_sdk_dynamodb::Client as DynamoClient;

use quilkin::cli::Cli;
use quilkin::filters::{session_router, FilterRegistry};
use quilkin::Result as QuilkinResult;

fn main() {
    // Build a multi-threaded Tokio runtime (the “old logic” approach).
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .thread_name_fn(|| {
            static ATOMIC_ID: AtomicUsize = AtomicUsize::new(0);
            let id = ATOMIC_ID.fetch_add(1, AtomicOrdering::SeqCst);
            format!("tokio-main-{id}")
        })
        .build()
        .expect("failed to build the tokio runtime");

    // Now we block_on the actual async logic.
    rt.block_on(async {
        // 1) Install stable_eyre for better error reporting
        stable_eyre::install().expect("failed to install stable_eyre");

        // 2) Load AWS config from environment, create a DynamoDB client
        let aws_conf = aws_config::load_from_env().await;
        let dynamo_client = DynamoClient::new(&aws_conf);

        // 3) Register your SessionRouter filter factory with Quilkin
        //    This uses `session_router::factory(Some(dynamo_client))`.
        FilterRegistry::register(vec![session_router::factory(Some(dynamo_client))]);

        // 4) Parse CLI args and drive the standard Quilkin flow
        let cli = Cli::parse();
        match cli.drive(None).await {
            Ok(_) => {
                info!("Quilkin shutting down normally");
                std::process::exit(0);
            }
            Err(e) => {
                error!(%e, error_debug=?e, "fatal error");
                std::process::exit(-1);
            }
        }
    });
}
