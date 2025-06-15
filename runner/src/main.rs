pub mod api_types;
pub mod runner;

use api_types::{
    ExecuteRequest, ExecuteResponse, Job, StartJobResponse, ViewJobResponse,
};
use axum::{
    extract::{Json, State},
    routing::{get, post},
    Router,
};
use log::{debug, error, info, trace, warn};

use reqwest::IntoUrl;
use runner::Runner;

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

struct AppState {
    sender: mpsc::Sender<Job>,
    runner: Runner,
}

async fn index() -> &'static str {
    concat!(
        "`GET /job` Gets all available jobs and their status\n",
        "`POST /job { <job_request> }` starts a new job from the request\n",
        "`POST /exec { <execute_request> }` Executes a series of commands from the request\n"
    )
}

async fn add_job(
    State(state): State<Arc<AppState>>,
    job: Json<Job>,
) -> Json<StartJobResponse> {
    let runner = &state.runner;
    runner.add_job(&state.sender, job.0);

    Json(StartJobResponse::new(
        *runner.busy().lock().unwrap(),
        *runner.waiting_jobs().lock().unwrap(),
    ))
}

async fn view_job(State(state): State<Arc<AppState>>) -> Json<ViewJobResponse> {
    let runner = &state.runner;

    Json(ViewJobResponse::new(
        *runner.busy().lock().unwrap(),
        *runner.waiting_jobs().lock().unwrap(),
    ))
}

async fn execute(
    State(state): State<Arc<AppState>>,
    execute_request: Json<ExecuteRequest>,
) -> Json<ExecuteResponse> {
    let runner = &state.runner;
    let responses = runner.run_commands(execute_request.0.commands());
    Json(ExecuteResponse::new(responses))
}

async fn status() {}

#[tokio::main]
async fn main() {
    if let Err(e) = dotenvy::dotenv() {
        eprintln!("Unable to find .env file, relying on host environment...");
    }
    env_logger::init();

    let (runner, sender) = Runner::new("localhost:50000".into());
    let state = Arc::new(AppState { runner, sender });

    let app = Router::new()
        .route("/", get(index))
        .route("/status", get(status))
        .route("/job", get(view_job).post(add_job))
        .with_state(state.clone())
        .route("/execute", post(execute))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind("0.0.0.0:50001")
        .await
        .unwrap();

    info!("Now serving at http://localhost:50001");
    axum::serve(listener, app).await.unwrap();
}

struct EnvArgs {
    rust_log: String,
    brain_endpoint: String,
}
