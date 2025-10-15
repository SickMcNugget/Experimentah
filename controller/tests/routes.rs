pub mod common;

use std::sync::Arc;

use controller::run::ExperimentRunner;

use crate::common::{read_in_default_configs, test_path};

#[tokio::test]
async fn run_route() {
    let (config, experiment_config) = read_in_default_configs();

    let mut state = ExperimentRunner::new();
    state.configuration.storage_dir = test_path();
    let state = Arc::new(state);

    controller::routes::run(
        axum::extract::State(state),
        axum::Json((config, experiment_config)),
    )
    .await
    .unwrap();
}

#[tokio::test]
async fn status_route() {}
