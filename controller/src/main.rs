use std::{env, sync::Arc};

use axum::{
    routing::{get, post},
    Router,
};

use controller::{
    routes::{self, RunState},
    run::ExperimentRunner,
};

#[allow(unused_imports)]
use log::{debug, error, info, trace, warn};

#[tokio::main]
async fn main() {
    let log_level: String = env::var("RUST_LOG").unwrap_or("info".to_string());
    env::set_var("RUST_LOG", log_level);
    env_logger::init();

    let address = "0.0.0.0";
    let port = 50000;
    let url = format!("{address}:{port}");

    let listener = tokio::net::TcpListener::bind(&url).await.unwrap();
    info!("Now serving at http://{}", &url);

    let runner = Arc::new(ExperimentRunner::new());
    let state_runner = runner.clone();

    let runner_handle = tokio::spawn(async move {
        runner.start().await;
    });

    let serve_handle = axum::serve(listener, router(state_runner));
    let (_serve_data, _runner_data) = tokio::join!(serve_handle, runner_handle);
}

// TODO: Add CLAP for argument parsing instead of using an env, as Controller may have to configure
// via command line args and cannot directly alter the env file on the runner's host.

fn router(run_state: RunState) -> Router {
    Router::new()
        .route("/", get(routes::index))
        .route("/status", get(routes::status))
        .route("/run", post(routes::run))
        .route("/upload", post(routes::upload))
        .with_state(run_state)
}
