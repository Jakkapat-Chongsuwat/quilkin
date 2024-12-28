//
// src/main.rs
//

use std::sync::atomic::{AtomicUsize, Ordering as AtomicOrdering};

use clap::Parser;
use stable_eyre; // For better error reporting
use tracing::{error, info};
use quilkin::cli::Cli;


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
