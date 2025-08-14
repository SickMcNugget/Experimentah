pub mod common;

use std::sync::Arc;

use controller::{
    parse::{Config, ExperimentConfig},
    run::ExperimentRunner,
};

use crate::common::{test_path, VALID_CONFIG, VALID_EXPERIMENT_CONFIG};

#[tokio::test]
async fn run_route() {
    let config = Config::from_file(test_path().join(VALID_CONFIG)).unwrap();
    config.validate().unwrap();
    let experiment_config =
        ExperimentConfig::from_file(test_path().join(VALID_EXPERIMENT_CONFIG))
            .unwrap();
    experiment_config.validate(&config).unwrap();

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
