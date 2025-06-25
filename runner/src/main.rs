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
    async fn oneshot_request(request: Request<Body>) -> Response<Body> {
        let state = create_state();
        let app = app(state);
        let response = app.oneshot(request).await.unwrap();
        response
    }
    #[tokio::test]
    async fn get_home() {
        let request = Request::builder().uri("/").body(Body::empty()).unwrap();

        let response = oneshot_request(request).await;
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
        let request = Request::builder()
            .uri("/experiment")
            .body(Body::empty())
            .unwrap();
        let response = oneshot_request(request).await;

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
    async fn add_valid_experiment() {
        let experiment_data = Experiment::new(
            String::from("test_experiment_1"),
            // This can be any file that exists in the fs.
            String::from("/tmp"),
            None,
        );
        let request = Request::builder()
            .method("POST")
            .uri("/experiment")
            .header("Content-Type", "application/json")
            .body(Body::from(serde_json::to_string(&experiment_data).unwrap()))
            .unwrap();

        let response = oneshot_request(request).await;

        println!("Add Experiment Received {}", response.status());
        assert_eq!(response.status(), StatusCode::OK);

        let body_str = axum_body_to_str(response.into_body()).await;
        println!("Add Experiment Response was {}", body_str);

        // Should be a view experiment response struct - test conversion doesn't fail
        let data: ExperimentResponse = serde_json::from_str(&body_str).unwrap();

        assert_eq!(data.busy, false);
        assert_eq!(data.waiting_experiments, 1);
    }

    // Populate with expected HTTP sender behaviour.
    #[tokio::test]
    async fn add_invalid_experiment() {
        // This should panic as the file doesn't exist (required by Experiment)
        let result = std::panic::catch_unwind(|| {
            let experiment_data = Experiment::new(
                String::from("test_experiment_1"),
                // This can be any file that doesn't exist in the fs.
                String::from("/abc/def/ghi.sh"),
                None,
            );
        });
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn run_valid_commands() {
        // simulate main by running the runner server
        let state = create_state();
        let app = app(state);
        let command_data = ExecuteRequest {
            commands: vec![
                String::from("echo \" $HOSTNAME 123 \""),
                String::from("ls"),
            ],
        };
        let request = Request::builder()
            .method("POST")
            .uri("/execute")
            .header("Content-Type", "application/json")
            .body(Body::from(serde_json::to_string(&command_data).unwrap()))
            .unwrap();

        let response = oneshot_request(request).await;

        println!("Run Commands Received {}", response.status());
        assert_eq!(response.status(), StatusCode::OK);

        let body_str = axum_body_to_str(response.into_body()).await;
        println!("Run Commands Response was {}", body_str);

        // Returns command stderr
        let data: ExecuteResponse = serde_json::from_str(&body_str).unwrap();

        assert_eq!(data.command_responses.len(), 2);
        for cmd_stderr in data.command_responses {
            assert_eq!(cmd_stderr, "");
        }
    }

    #[tokio::test]
    async fn run_invalid_commands() {
        // simulate main by running the runner server
        let state = create_state();
        let app = app(state);
        let command_data = ExecuteRequest {
            commands: vec![
                String::from("surelythisisntarealcommand"),
                String::from("cd -cool"),
            ],
        };
        let request = Request::builder()
            .method("POST")
            .uri("/execute")
            .header("Content-Type", "application/json")
            .body(Body::from(serde_json::to_string(&command_data).unwrap()))
            .unwrap();

        let response = oneshot_request(request).await;

        println!("Run Commands Received {}", response.status());
        assert_eq!(response.status(), StatusCode::OK);

        let body_str = axum_body_to_str(response.into_body()).await;
        println!("Run Commands Response was {}", body_str);

        // Returns command stderr
        let data: ExecuteResponse = serde_json::from_str(&body_str).unwrap();
        assert_eq!(data.command_responses.len(), 2);
        assert!(data.command_responses[0].contains("command not found"));
        assert!(data.command_responses[1].contains("invalid option"));
    }
}
