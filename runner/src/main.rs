// pub mod api_types;
// pub mod runner;
// pub mod utils;

// use api_types::{
//     ExecuteRequest, ExecuteResponse, Experiment, ExperimentResponse,
// };
use axum::{
    extract::{Json, State},
    routing::{get, post},
    Router,
};
use log::{debug, error, info, trace, warn};

use reqwest::IntoUrl;
// use runner::Runner;

use std::sync::{mpsc, Arc};
// use reqwest::Client;
//
// use std::{
//     collections::HashMap,
//     error::Error,
//     fmt,
//     process::Command,
//     sync::{mpsc, Mutex},
//     time::{SystemTime, UNIX_EPOCH},
// };

//use rocket::{serde::json::Json, State};

// TODO: Add CLAP for argument parsing instead of using an env, as Controller may have to configure
// via command line args and cannot directly alter the env file on the runner's host.

struct AppState {
    sender: mpsc::Sender<Experiment>,
    runner: Runner,
}

async fn index() -> &'static str {
    concat!(
        "`GET /experiment` Gets all available experiments and their status\n",
        "`POST /experiment { <experiment_request> }` starts a new experiment from the request\n",
        "`POST /exec { <execute_request> }` Executes a series of commands from the request\n"
    )
}

/// Adds a new experiment to the runner queue.
/// Experiments are started at the runner's discretion.
async fn add_experiment(
    State(state): State<Arc<AppState>>,
    experiment: Json<Experiment>,
) -> Json<ExperimentResponse> {
    let runner = &state.runner;
    runner.add_experiment(&state.sender, experiment.0);

    Json(ExperimentResponse {
        busy: *runner.busy().lock().unwrap(),
        waiting_experiments: *runner.waiting_experiments().lock().unwrap(),
    })
}

async fn view_experiment(
    State(state): State<Arc<AppState>>,
) -> Json<ExperimentResponse> {
    let runner = &state.runner;

    Json(ExperimentResponse {
        busy: *runner.busy().lock().unwrap(),
        waiting_experiments: *runner.waiting_experiments().lock().unwrap(),
    })
}

/// Used to run discrete, one-off bash commands.
async fn execute(
    State(state): State<Arc<AppState>>,
    execute_request: Json<ExecuteRequest>,
) -> Json<ExecuteResponse> {
    let runner = &state.runner;
    let responses = runner.run_commands(&execute_request.0.commands);
    Json(ExecuteResponse {
        command_responses: responses,
    })
}

async fn status() {}

fn app(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/", get(index))
        .route("/status", get(status))
        .route("/experiment", get(view_experiment).post(add_experiment))
        .with_state(state.clone())
        .route("/execute", post(execute))
        .with_state(state)
}

fn create_state() -> Arc<AppState> {
    let (runner, sender) = Runner::new("localhost:50000".into());
    Arc::new(AppState { runner, sender })
}

async fn run_server() {
    let listener = tokio::net::TcpListener::bind("0.0.0.0:50001")
        .await
        .unwrap();

    info!("Now serving at http://localhost:50001");
    axum::serve(listener, app(create_state())).await.unwrap();
}

#[tokio::main]
async fn main() {
    if let Err(e) = dotenvy::dotenv() {
        eprintln!("Unable to find .env file, relying on host environment...");
    }
    env_logger::init();
    run_server().await;
}
