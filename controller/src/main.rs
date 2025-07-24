use std::{
    env,
    sync::{atomic::Ordering, Arc},
};

use axum::{
    extract::State,
    routing::{get, post},
    Json, Router,
};

use controller::{
    parse::{self, Config, ExperimentConfig, ParseError},
    run::ExperimentRunner,
};

#[allow(unused_imports)]
use log::{debug, error, info, trace, warn};

type SharedRunState = Arc<RunState>;

struct RunState {
    runner: Arc<ExperimentRunner>,
}

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

    let run_state = Arc::new(RunState {
        runner: state_runner,
    });

    let serve_handle = axum::serve(listener, router(run_state));
    let (_serve_data, _runner_data) = tokio::join!(serve_handle, runner_handle);
}

// TODO: Add CLAP for argument parsing instead of using an env, as Controller may have to configure
// via command line args and cannot directly alter the env file on the runner's host.

async fn index() -> &'static str {
    concat!(
        "`GET /` Gets all available experiments and their status\n",
        "`GET /status` Retrieves the current status of the runner. This shows if it is still running an experiment.\n",
        "`POST /run { <execute_request> }` Executes a series of commands from the request\n"
    )
}

//
/// Adds a new experiment to the runner queue.
/// Experiments are started at the runner's discretion.
// async fn add_experiment(
//     State(state): State<Arc<AppState>>,
//     experiment: Json<Experiment>,
// ) -> Json<ExperimentResponse> {
//     let runner = &state.runner;
//     runner.add_experiment(&state.sender, experiment.0);
//
//     Json(ExperimentResponse {
//         busy: *runner.busy().lock().unwrap(),
//         waiting_experiments: *runner.waiting_experiments().lock().unwrap(),
//     })
// }

// async fn view_experiment(
//     State(state): State<Arc<AppState>>,
// ) -> Json<ExperimentResponse> {
//     let runner = &state.runner;
//
//     Json(ExperimentResponse {
//         busy: *runner.busy().lock().unwrap(),
//         waiting_experiments: *runner.waiting_experiments().lock().unwrap(),
//     })
// }

/// Used to run discrete, one-off bash commands.
// async fn execute(
//     State(state): State<Arc<AppState>>,
//     execute_request: Json<ExecuteRequest>,
// ) -> Json<ExecuteResponse> {
//     let runner = &state.runner;
//     let responses = runner.run_commands(&execute_request.0.commands);
//     Json(ExecuteResponse {
//         command_responses: responses,
//     })
// }

// async fn status() {}

fn router(run_state: SharedRunState) -> Router {
    Router::new()
        .route("/", get(index))
        .route("/status", get(status))
        .route("/run", post(run))
        .with_state(run_state)
}

// fn create_state<'a>() -> Arc<RunState<'a>> {
//     let runner = ExperimentRunner::new();
//     Arc::new(RunState { runner })
// }

// #[axum::debug_handler]
async fn run(
    State(run_state): State<SharedRunState>,
    configs: Json<(Config, ExperimentConfig)>,
) -> Result<(), ParseError> {
    info!("Received configs over the wire");
    let (config, experiment_config) = configs.0;

    config.validate()?;
    experiment_config.validate(&config)?;
    info!("Successfully validated configs");

    let experiments = parse::generate_experiments(&config, &experiment_config);
    info!("Generated experiments from configs");

    run_state.runner.enqueue(experiments).await;
    info!("Enqueued experiments into the runner");

    Ok(())
}

async fn status(State(run_state): State<SharedRunState>) -> String {
    let runner = &run_state.runner;
    let current_experiment = runner.current_experiment.lock().await;
    let current_run = runner.current_run.load(Ordering::Relaxed);
    let current_runs = runner.current_runs.load(Ordering::Relaxed);

    format!("Current Experiment: {current_experiment:?}, Current run: {current_run:?}/{current_runs:?}")
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use axum::{
        body::{to_bytes, Body},
        http::{response::Response, Request, StatusCode},
        routing::Router,
    };
    use tower::ServiceExt as _;

    async fn axum_body_to_str(body: Body) -> String {
        let bytes = to_bytes(body, usize::MAX).await.unwrap();
        String::from_utf8(bytes.to_vec()).unwrap()
    }

    async fn oneshot_request(request: Request<Body>) -> Response<Body> {
        let router = setup();
        router.oneshot(request).await.unwrap()
    }

    fn setup() -> Router {
        let run_state = Arc::new(RunState {
            runner: Arc::new(ExperimentRunner::new()),
        });

        router(run_state)
    }

    #[tokio::test]
    async fn get_home() {
        let request = Request::builder().uri("/").body(Body::empty()).unwrap();

        let response = oneshot_request(request).await;
        println!("Received {}", response.status());
        assert_eq!(response.status(), StatusCode::OK);

        let body_str = axum_body_to_str(response.into_body()).await;
        println!("Response was {}", body_str);
        let search_strs = vec!["GET /", "GET /status", "POST /run"];
        for str in search_strs {
            assert!(body_str.contains(str));
        }
    }

    #[tokio::test]
    async fn post_run_endpoint() {
        // TODO(joren): Move these kinds of integration tests into the tests/ folder
        let test = PathBuf::from("./test");
        let config_data =
            Config::from_file(&test.join("valid_config.toml")).unwrap();
        let experiment_config_data = ExperimentConfig::from_file(
            &test.join("valid_experiment_config.toml"),
        )
        .unwrap();

        let request = Request::builder()
            .method("POST")
            .uri("/run")
            .header("Content-Type", "application/json")
            .body(Body::from(
                serde_json::to_string::<(Config, ExperimentConfig)>(&(
                    config_data,
                    experiment_config_data,
                ))
                .unwrap(),
            ))
            .unwrap();

        let response = oneshot_request(request).await;
        let status = response.status();
        assert_eq!(
            status,
            StatusCode::OK,
            "Expected a successful request, got status: {status}"
        );
    }

    // #[tokio::test]
    // async fn get_experiment_endpoint() {
    //     let request = Request::builder()
    //         .uri("/experiment")
    //         .body(Body::empty())
    //         .unwrap();
    //     let response = oneshot_request(request).await;
    //
    //     println!("Received {}", response.status());
    //     assert_eq!(response.status(), StatusCode::OK);
    //
    //     let body_str = axum_body_to_str(response.into_body()).await;
    //     println!("/Experiment Response was {}", body_str);
    //
    //     // Should be a view experiment response struct - test conversion doesn't fail
    //     let data: ExperimentResponse = serde_json::from_str(&body_str).unwrap();
    //
    //     assert_eq!(data.busy, false);
    //     assert_eq!(data.waiting_experiments, 0);
    // }

    // #[tokio::test]
    // async fn add_valid_experiment() {
    //     let experiment_data = Experiment::new(
    //         String::from("test_experiment_1"),
    //         // This can be any file that exists in the fs.
    //         String::from("/tmp"),
    //         None,
    //     );
    //     let request = Request::builder()
    //         .method("POST")
    //         .uri("/experiment")
    //         .header("Content-Type", "application/json")
    //         .body(Body::from(serde_json::to_string(&experiment_data).unwrap()))
    //         .unwrap();
    //
    //     let response = oneshot_request(request).await;
    //
    //     println!("Add Experiment Received {}", response.status());
    //     assert_eq!(response.status(), StatusCode::OK);
    //
    //     let body_str = axum_body_to_str(response.into_body()).await;
    //     println!("Add Experiment Response was {}", body_str);
    //
    //     // Should be a view experiment response struct - test conversion doesn't fail
    //     let data: ExperimentResponse = serde_json::from_str(&body_str).unwrap();
    //
    //     assert_eq!(data.busy, false);
    //     assert_eq!(data.waiting_experiments, 1);
    // }

    // // Populate with expected HTTP sender behaviour.
    // #[tokio::test]
    // async fn add_invalid_experiment() {
    //     // This should panic as the file doesn't exist (required by Experiment)
    //     let result = std::panic::catch_unwind(|| {
    //         let experiment_data = Experiment::new(
    //             String::from("test_experiment_1"),
    //             // This can be any file that doesn't exist in the fs.
    //             String::from("/abc/def/ghi.sh"),
    //             None,
    //         );
    //     });
    //     assert!(result.is_err());
    // }

    // #[tokio::test]
    // async fn run_valid_commands() {
    //     // simulate main by running the runner server
    //     let state = create_state();
    //     let app = app(state);
    //     let command_data = ExecuteRequest {
    //         commands: vec![
    //             String::from("echo \" $HOSTNAME 123 \""),
    //             String::from("ls"),
    //         ],
    //     };
    //     let request = Request::builder()
    //         .method("POST")
    //         .uri("/execute")
    //         .header("Content-Type", "application/json")
    //         .body(Body::from(serde_json::to_string(&command_data).unwrap()))
    //         .unwrap();
    //
    //     let response = oneshot_request(request).await;
    //
    //     println!("Run Commands Received {}", response.status());
    //     assert_eq!(response.status(), StatusCode::OK);
    //
    //     let body_str = axum_body_to_str(response.into_body()).await;
    //     println!("Run Commands Response was {}", body_str);
    //
    //     // Returns command stderr
    //     let data: ExecuteResponse = serde_json::from_str(&body_str).unwrap();
    //
    //     assert_eq!(data.command_responses.len(), 2);
    //     for cmd_stderr in data.command_responses {
    //         assert_eq!(cmd_stderr, "");
    //     }
    // }

    // #[tokio::test]
    // async fn run_invalid_commands() {
    //     // simulate main by running the runner server
    //     let state = create_state();
    //     let app = app(state);
    //     let command_data = ExecuteRequest {
    //         commands: vec![
    //             String::from("surelythisisntarealcommand"),
    //             String::from("cd -cool"),
    //         ],
    //     };
    //     let request = Request::builder()
    //         .method("POST")
    //         .uri("/execute")
    //         .header("Content-Type", "application/json")
    //         .body(Body::from(serde_json::to_string(&command_data).unwrap()))
    //         .unwrap();
    //
    //     let response = oneshot_request(request).await;
    //
    //     println!("Run Commands Received {}", response.status());
    //     assert_eq!(response.status(), StatusCode::OK);
    //
    //     let body_str = axum_body_to_str(response.into_body()).await;
    //     println!("Run Commands Response was {}", body_str);
    //
    //     // Returns command stderr
    //     let data: ExecuteResponse = serde_json::from_str(&body_str).unwrap();
    //     assert_eq!(data.command_responses.len(), 2);
    //     assert!(data.command_responses[0].contains("command not found"));
    //     assert!(data.command_responses[1].contains("invalid option"));
    // }
}
