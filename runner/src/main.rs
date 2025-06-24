pub mod api_types;
pub mod runner;
pub mod utils;

use api_types::{
    ExecuteRequest, ExecuteResponse, Experiment, ExperimentResponse,
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

async fn add_experiment(
    State(state): State<Arc<AppState>>,
    experiment: Json<Experiment>,
) -> Json<ExperimentResponse> {
    let runner = &state.runner;
    runner.add_experiment(&state.sender, experiment.0);

    Json(ExperimentResponse::new(
        *runner.busy().lock().unwrap(),
        *runner.waiting_experiments().lock().unwrap(),
    ))
}

async fn view_experiment(
    State(state): State<Arc<AppState>>,
) -> Json<ExperimentResponse> {
    let runner = &state.runner;

    Json(ExperimentResponse::new(
        *runner.busy().lock().unwrap(),
        *runner.waiting_experiments().lock().unwrap(),
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

struct EnvArgs {
    rust_log: String,
    brain_endpoint: String,
}
// TODO: Just for my debugging - can remove later
fn print_type<T>(_: &T) {
    println!("{:?}", std::any::type_name::<T>());
}
#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::{to_bytes, Body, Bytes},
        extract::connect_info::MockConnectInfo,
        http::{self, Request, StatusCode},
    };
    use http::response::Response;
    use tower::{Service, ServiceExt};

    async fn axum_body_to_str(body: Body) -> String {
        let bytes = to_bytes(body, usize::MAX).await.unwrap();
        String::from_utf8(bytes.to_vec()).unwrap()
    }
    #[tokio::test]
    async fn get_home() {
        // simulate main by running the runner server
        let state = create_state();
        let app = app(state);
        let request = Request::builder().uri("/").body(Body::empty()).unwrap();
        let response = app.oneshot(request).await.unwrap();

        println!("Received {}", response.status());
        assert_eq!(response.status(), StatusCode::OK);

        let body_str = axum_body_to_str(response.into_body()).await;
        println!("Response was {}", body_str);
        let search_strs =
            vec!["GET /experiment", "POST /experiment", "POST /exec"];
        for str in search_strs {
            assert!(body_str.contains(str));
        }
    }

    #[tokio::test]
    async fn get_experiment_endpoint() {
        // simulate main by running the runner server
        let state = create_state();
        let app = app(state);
        let request = Request::builder()
            .uri("/experiment")
            .body(Body::empty())
            .unwrap();
        let response = app.oneshot(request).await.unwrap();

        println!("Received {}", response.status());
        assert_eq!(response.status(), StatusCode::OK);

        let body_str = axum_body_to_str(response.into_body()).await;
        println!("/Experiment Response was {}", body_str);

        // Should be a view experiment response struct - test conversion doesn't fail
        let data: ExperimentResponse = serde_json::from_str(&body_str).unwrap();

        assert_eq!(data.busy, false);
        assert_eq!(data.waiting_experiments, 0);
    }

    #[tokio::test]
    async fn add_experiment() {
        // simulate main by running the runner server
        let state = create_state();
        let app = app(state);
        let experiment_data = Experiment::new(
            String::from("test_experiment_1"),
            String::from("test/abc.sh"),
            None,
        );
        let request = Request::builder()
            .method("POST")
            .uri("/experiment")
            .header("Content-Type", "application/json")
            .body(Body::from(serde_json::to_string(&experiment_data).unwrap()))
            .unwrap();

        let response = app.oneshot(request).await.unwrap();

        println!("Add Experiment Received {}", response.status());
        assert_eq!(response.status(), StatusCode::OK);

        let body_str = axum_body_to_str(response.into_body()).await;
        println!("Add Experiment Response was {}", body_str);

        // Should be a view experiment response struct - test conversion doesn't fail
        let data: ExperimentResponse = serde_json::from_str(&body_str).unwrap();

        assert_eq!(data.busy, false);
        assert_eq!(data.waiting_experiments, 1);
    }

    #[tokio::test]
    async fn run_experiment() {
        // simulate main by running the runner server
        let state = create_state();
        let app = app(state);
        // let experiment_data = Experiment {
        //     id: String::from("test_experiment_1"),
        //     workload: String::from("test/abc.sh"),
        //     arguments: None,
        // };
        // let request = Request::builder()
        //     .method("POST")
        //     .uri("/experiment")
        //     .header("Content-Type", "application/json")
        //     .body(Body::from(serde_json::to_string(&experiment_data).unwrap()))
        //     .unwrap();
        //
        // let response = app.oneshot(request).await.unwrap();
        //
        // println!("Add Experiment Received {}", response.status());
        // assert_eq!(response.status(), StatusCode::OK);
        //
        // let body_str = axum_body_to_str(response.into_body()).await;
        // println!("Add Experiment Response was {}", body_str);
        //
        // // Should be a view experiment response struct - test conversion doesn't fail
        // let data: ExperimentResponse = serde_json::from_str(&body_str).unwrap();
        //
        // assert_eq!(data.busy, false);
        // assert_eq!(data.waiting_experiments, 1);
    }
}
