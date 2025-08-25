use std::{
    os::unix::fs::PermissionsExt, path::PathBuf, str::FromStr, sync::Arc,
};

use axum::{
    body::Bytes,
    extract::{
        ws::{Message, Utf8Bytes, WebSocket},
        Multipart, State, WebSocketUpgrade,
    },
    response::Response,
    Json,
};
use log::info;
use tokio::sync::Mutex;

use crate::{
    parse::{self, Config, ExperimentConfig, FileType},
    run::ExperimentRunner,
    DEFAULT_STORAGE_DIR,
};

pub type RunState = Arc<ExperimentRunner>;

/// An endpoint that shows all the other endpoints, with a short description of each
pub async fn index() -> &'static str {
    concat!(
        "`GET /` Gets all available experiments and their status\n",
        "`GET /status` Retrieves the current status of the runner. This shows if it is still running an experiment.\n",
        "`POST /run { <execute_request> }` Executes a series of commands from the request\n",
        "`POST /upload { <files> }` Uploads a list of local files to the controller, storing them in a well-known location",
    )
}

/// The main endpoint for Experimentah. In this endpoint, a [`Config`] and an [`ExperimentConfig`]
/// are received, validated, converted into [`parse::Experiment`]s, and then enqueued inside of the [`crate::run::ExperimentRunner`].
pub async fn run(
    State(run_state): State<RunState>,
    configs: Json<(Config, ExperimentConfig)>,
) -> Result<(), parse::Error> {
    info!("Received configs over the wire");
    let (config, experiment_config) = configs.0;

    config.validate()?;
    experiment_config.validate(&config)?;
    experiment_config.validate_files(&run_state.configuration.storage_dir)?;
    info!("Successfully validated configs");

    let experiments = parse::generate_experiments(
        &config,
        &experiment_config,
        &run_state.configuration.storage_dir,
    )?;
    info!("Generated experiments from configs");

    let runner = &run_state;
    runner.enqueue(experiments).await;
    info!("Enqueued experiments into the runner");

    Ok(())
}

/// Retrieves the current state of the [`crate::run::ExperimentRunner`].
pub async fn status(State(run_state): State<RunState>) -> String {
    let runner = &run_state;

    runner.current.status().await
    // let current_experiment = runner.current_experiment.lock().await;
    // let current_run = runner.current_run.load(Ordering::Relaxed);
    // let current_runs = runner.current_runs.load(Ordering::Relaxed);
    //
    // format!("Current Experiment: {current_experiment:?}, Current run: {current_run:?}/{current_runs:?}")
}

pub async fn upload(mut multipart: Multipart) -> Result<(), String> {
    let mut path: PathBuf = PathBuf::from(DEFAULT_STORAGE_DIR);
    let mut file: Option<Bytes> = None;

    while let Some(field) = multipart.next_field().await.unwrap() {
        let name = field.name().expect("No name sent as part of multipart");
        match name {
            "type" => {
                let ft =
                    FileType::from_str(field.text().await.unwrap().as_str())
                        .unwrap();
                path.push(ft.to_string());
            }
            "file" => {
                path.push(field.file_name().unwrap());
                file = Some(field.bytes().await.unwrap());
            }
            _ => {
                panic!("Invalid multipart fields received");
            }
        }
    }

    let dir = path.parent().expect(
        "Expected a path containing directories and ending with a file",
    );
    std::fs::create_dir_all(dir).expect("Failed to create directory");
    std::fs::write(&path, file.unwrap()).expect("Failed to write file to disk");

    // We want to make sure our files are executable if it's a shell script
    // TODO(joren): I've just bodged it for now, but surely there is a smarter way to determine
    // whether something needs to be executable. I'm not against a LUT, though.
    if let Some(extension) = path.extension() {
        if extension == "sh" {
            let mut perms = std::fs::metadata(&path).unwrap().permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(&path, perms).unwrap();
        }
    }
    info!("Saved file {:?} to {:?}", path.file_name().unwrap(), path);

    Ok(())
}

pub async fn ws_handler(
    ws: WebSocketUpgrade,
    State(run_state): State<RunState>,
) -> Response {
    ws.on_upgrade(|socket| handle_socket(socket, run_state))
}

async fn handle_socket(mut socket: WebSocket, runner: RunState) {
    // We receive one message on this websocket?
    if let Some(msg) = socket.recv().await {
        if let Ok(msg) = msg {
            process_message(msg);
        }
    } else {
        eprintln!("Websocket abruptly closed");
    }

    // Assume after we process the message that we now have both a host and a filename/exporter
    // name

    let thing = Thing {
        host: String::from("root@prod-agent1.recsa.prod"),
        filetype: FileType::Exporter,
        identifier: String::from("sar-exporter"),
    };

    // We need to setup a *follow* for the file requested, that allows us to tune into the
    // stdout/stderr of the file, which we can stream as required.
    let (mut stdout, mut stderr) = match runner.tail_stdout_stderr(thing).await
    {
        Ok((stdout, stderr)) => (stdout, stderr),
        Err(e) => match e {
            crate::run::Error::FollowError(error) => {
                eprintln!("{error}");
                socket.send(Message::Close(None)).await.unwrap();
                return;
            }
            _ => panic!("{e}"),
        },
    };

    // Splitting seems to kill our socket.

    let socket_stdout = Arc::new(Mutex::new(socket));
    let socket_stderr = Arc::clone(&socket_stdout);

    let stdout_handle = tokio::spawn(async move {
        while let Some(msg) = stdout.recv().await {
            match socket_stdout.lock().await.send(msg).await {
                Ok(()) => {}
                Err(_e) => break,
            }
        }
    });

    let stderr_handle = tokio::spawn(async move {
        while let Some(msg) = stderr.recv().await {
            match socket_stderr.lock().await.send(msg).await {
                Ok(()) => {}
                Err(_e) => break,
            }
        }
    });

    println!("Waiting on handles to send to the websocket");
    // dbg!(&stdout_handle, &stderr_handle);

    let (ret1, ret2) = tokio::join!(stdout_handle, stderr_handle);
    ret1.unwrap();
    ret2.unwrap();
    println!("Handles joined!");
    // tokio::join!(stdout_handle);
}

pub struct Thing {
    pub host: String,
    pub identifier: String,
    pub filetype: FileType,
}

fn process_message(msg: Message) {
    match msg {
        Message::Close(c) => {
            if let Some(cf) = c {
                println!(
                    ">>> Received a close message with code {} and reason `{}`",
                    cf.code, cf.reason
                );
            } else {
                eprintln!(
                    ">>> Somehow a close message was sent without a closeframe"
                );
            }
        }
        Message::Binary(d) => {
            println!(">>> sent {} bytes: {d:?}", d.len());
        }
        Message::Text(d) => {
            println!("Yay!: {d}");
        }
        _ => {
            panic!("Unexpected message type received");
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::routes::index;

    #[tokio::test]
    async fn route_index() {
        let ret = index().await;
        let search_strs = vec!["GET /", "GET /status", "POST /run"];
        for str in search_strs {
            assert!(ret.contains(str));
        }
    }
    //
    // #[tokio::test]
    // async fn post_run_endpoint() {
    //     // TODO(joren): Move these kinds of integration tests into the tests/ folder
    //     let test = PathBuf::from("./test");
    //     let config_data =
    //         Config::from_file(&test.join("valid_config.toml")).unwrap();
    //     let experiment_config_data = ExperimentConfig::from_file(
    //         &test.join("valid_experiment_config.toml"),
    //     )
    //     .unwrap();
    //
    //     let request = Request::builder()
    //         .method("POST")
    //         .uri("/run")
    //         .header("Content-Type", "application/json")
    //         .body(Body::from(
    //             serde_json::to_string::<(Config, ExperimentConfig)>(&(
    //                 config_data,
    //                 experiment_config_data,
    //             ))
    //             .unwrap(),
    //         ))
    //         .unwrap();
    //
    //     let response = oneshot_request(request).await;
    //     let status = response.status();
    //     assert_eq!(
    //         status,
    //         StatusCode::OK,
    //         "Expected a successful request, got status: {status}"
    //     );
    // }
}
