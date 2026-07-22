//! Native `image` VGI worker binary.
//!
//! A standalone executable DuckDB launches and talks to over Apache Arrow IPC
//! (`ATTACH 'img' (TYPE vgi, LOCATION '…')`). All function registration lives in
//! the library crate so the wasm build serves an identical worker.

fn main() {
    // Logs MUST go to stderr — stdout is the Arrow-IPC channel.
    let _ = env_logger::Builder::from_env(env_logger::Env::default().filter_or("VGI_LOG", "info"))
        .format_timestamp_millis()
        .try_init();

    image_worker::build_worker().run();
}
