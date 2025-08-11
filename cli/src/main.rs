// Written by Joren Regan
// Date: 2025-06-13
//
// This is the client-side portion of the Experimentah system.
// Two threads are at play (once validation has succeeded and the system is ready):
// 1. The experiment running thread. Tells the runner what to do and waits until it's done.
// 2. The heartbeat thread. Ensures that endpoints continue to be reachable. If they aren't,
// after a time delay, notifies the main thread which then:
//  a. Tries to shut down the runner, collecting whatever results may exist.
//  b. Tries to upload collected results (if the error was with repo node)
//  c. Dies

use std::path::PathBuf;
use std::{env, str::FromStr};

use clap::{Args, Parser};

use controller::parse::{Config, ExperimentConfig, FileType};

use reqwest::blocking::{multipart, Client};

use std::time::Duration;

use cli::CliError;

// const HEARTBEAT_INTERVAL: Duration = Duration::from_millis(5000);

fn arg_or_error<'a, T>(
    arg: &'a Option<T>,
    ident: &'static str,
) -> Result<&'a T, CliError> {
    match arg.as_ref() {
        None => Err(CliError::from(format!("{ident} was not set"))),
        Some(val) => Ok(val),
    }
}

// #[tokio::main]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = AppArgs::parse();

    let address = args.address;
    let timeout = Duration::from_secs(args.timeout);

    std::env::set_current_dir(args.working_directory)?;

    let client = reqwest::blocking::Client::builder()
        .timeout(timeout)
        .build()?;

    // Early exit if we just want a status check
    if args.pathway.status {
        check_status(&client, &address)?;
        return Ok(());
    } else if args.pathway.validate {
        validate_configs(&args.config, &args.experiment_config);
        return Ok(());
    } else if !args.pathway.upload.is_empty() {
        upload_data(&client, &address, args.pathway.upload)?;
        return Ok(());
    }

    let config = arg_or_error(&args.config, "CONFIG_FILE")?;
    let experiment_config =
        arg_or_error(&args.experiment_config, "EXPERIMENT_CONFIG_FILE")?;

    let config: Config = Config::from_file(config)?;
    println!("Successfully parsed config");
    // TODO(joren): This shouldn't really return Ok()
    match config.validate() {
        Ok(()) => println!("Successfully validated config"),
        Err(e) => {
            eprintln!("{e}");
            return Ok(());
        }
    }
    // println!("Successfully validated config");

    let experiment_config: ExperimentConfig =
        ExperimentConfig::from_file(experiment_config)?;
    println!("Successfully parsed experiment config");

    // TODO(joren): This shouldn't really return Ok()
    match experiment_config.validate(&config) {
        Ok(()) => println!("Successfully validated experiment_config"),
        Err(e) => {
            eprintln!("{e}");
            return Ok(());
        }
    }

    let res = client
        .post(format!("http://{address}/run"))
        .json(&(config, experiment_config))
        .send()?;
    dbg!(res);
    Ok(())
}

fn check_status(client: &Client, address: &str) -> cli::Result<()> {
    let response = client
        .get(format!("http://{address}/status"))
        .send()
        .map_err(|e| CliError::from(("Error checking status", e)))?;

    // dbg!(&response);

    // let json: String = response.json()?;
    let text: String = response.text()?;
    println!("{}", text);
    Ok(())
}

fn validate_configs(
    config: &Option<PathBuf>,
    experiment_config: &Option<PathBuf>,
) {
    if config.is_none() && experiment_config.is_some() {
        eprintln!(
            "Unable to validate an experiment config without a normal config"
        );
        return;
    }

    if config.is_none() && experiment_config.is_none() {
        eprintln!("No configs provided for validation");
        return;
    }

    // We've guaranteed a config is present.
    let config = config.as_ref().unwrap();
    match Config::from_file(config) {
        Ok(config) => match config.validate() {
            Ok(_) => {
                println!("Config is valid");
                match experiment_config {
                    Some(experiment_config) => {
                        match ExperimentConfig::from_file(experiment_config) {
                            Ok(experiment_config) => {
                                match experiment_config.validate(&config) {
                                    Ok(_) => {
                                        println!("Experiment config is valid");
                                    }
                                    Err(e) => {
                                        eprintln!(
                                            "Invalid experiment config: {}",
                                            e
                                        );
                                    }
                                }
                            }
                            Err(e) => {
                                eprintln!("Invalid experiment config: {}", e)
                            }
                        }
                    }
                    None => {
                        println!(
                            "Skipping Experiment config as it was not provided"
                        )
                    }
                }
            }
            Err(e) => {
                eprintln!("Invalid config: {e}")
            }
        },
        Err(e) => {
            eprintln!("Invalid config: {e}")
        }
    };
}

/// We make a few assumptions regarding uploaded files for ease-of-use
/// on the part of the user.
///     1. If a file contains setup/teardown, then it is probably a setup/teardown script.
///     2. If they do not pass in a type (setup/teardown/execute), it will be treated as an execution script.
fn parse_upload(s: &str) -> Result<(PathBuf, FileType), String> {
    let s = s.trim().replace("  ", " ");
    let parts: Vec<&str> = s.splitn(2, " ").collect();

    let filepath: PathBuf;
    let filetype: FileType;

    match parts.len() {
        0 => return Err("No file passed in".to_string()),
        1 => {
            filepath = PathBuf::from(parts[0])
                .canonicalize()
                .expect("Unable to canonicalize filepath");

            filetype = FileType::try_from(filepath.as_path()).unwrap();
        }
        2 => {
            filepath = PathBuf::from(parts[0])
                .canonicalize()
                .expect("Unable to canonicalize filepath");

            filetype = FileType::from_str(parts[1]).unwrap();
        }
        _ => {
            panic!("We should not have received a parts vector with more than 2 members");
        }
    }
    Ok((filepath, filetype))
}

fn upload_data(
    client: &Client,
    address: &str,
    blobs: Vec<(PathBuf, FileType)>,
) -> Result<(), CliError> {
    for (filepath, filetype) in blobs.into_iter() {
        let form = multipart::Form::new()
            .text("type", filetype.to_string())
            .file("file", filepath)?;

        let response = client
            .post(format!("http://{address}/upload"))
            .multipart(form)
            .send()
            .map_err(|e| ("Error checking status", e))?;
        dbg!(response);
    }
    Ok(())
}

// fn check

// println!("Logging to file: {:?}", args.log_file);

// let client = Client::builder()
//     .timeout(timeout)
//     .build()
//     .expect("Somehow an invalid client was built");

// let mut er = ExperimentRunner::new(
//     config.as_ref(),
//     experiment_config.as_ref(),
//     client.clone(),
// );

// I think that I want the controller to ensure that runners are available on the hosts they're
// meant to be on.
// I want the runner to ensure that exporters are working as expected, though, meaning that
// an endpoint needs to be added to the runner which allows an exporter to be registered (and
// subsequently monitored) by that runner. Exporters will be programs that are run in the
// background during the execution of a run. We'll use and advisory lock to make sure that
// multiple instances of the exporter program are not created, and that they're properly killed
// at the end of a run. I think the expectation at this point is that an exporter should be a
// long-running process which outputs it's data to a file. It's the responsibility of the
// exporter itself to include timestamp information and do all the heavy lifting regarding
// metrics collection.
// Think tools like sar, and raritan (my bespoke power usage script).

// TODO(joren): In the future, it may be better to only prepare runners for the experiment
// that's about to be run. For now, I'd like to do everything at the beginning, though.
// println!("Ensuring all runners are up and reachable");
// if let Err(e) = er.check_runners().await {
//     panic!("{e}");
// }
//
// tokio::spawn(async move { heartbeat_thread(client, config).await });
//
// let runs = &experiment_config.runs;
// println!("= Running Experiment {}=", runs);
//
// for _ in 0..*runs {
//     todo!();
// }
//
// println!("\n= Successful Experiments =");
// }

// Runs periodically and determines service connectivity
// async fn heartbeat_thread(client: Client, config: Arc<Config>) {
//     loop {
//         let brain_promise = client
//             .get(format!("{}/status", config.brain_endpoint()))
//             .send();
//
//         let mut runner_promises = Vec::with_capacity(config.runners.len());
//         for runner in config.runner_endpoints() {
//             runner_promises
//                 .push(client.get(format!("{}/status", runner)).send());
//         }
//
//         match brain_promise.await {
//             Ok(response) => match response.status() {
//                 StatusCode::OK => {}
//                 _ => eprintln!(
//                     "Bad status code from brain: {}",
//                     response.status()
//                 ),
//             },
//             Err(..) => {
//                 eprintln!("Error awaiting brain");
//             }
//         }
//         for runner_promise in runner_promises {
//             match runner_promise.await {
//                 Ok(response) => match response.status() {
//                     StatusCode::OK => {}
//                     _ => eprintln!(
//                         "Bad status code from runner: {}",
//                         response.status()
//                     ),
//                 },
//                 Err(..) => {
//                     eprintln!("Error awaiting runner");
//                 }
//             }
//         }
//         tokio::time::sleep(HEARTBEAT_INTERVAL).await;
//     }
// }

fn default_log_file() -> PathBuf {
    let mut path = env::current_exe().unwrap();
    path.pop();
    path.push("log/experimentah.log");
    path
}

fn default_working_directory() -> PathBuf {
    env::current_dir().unwrap()
}

#[derive(Parser, Debug)]
#[command(name = "Controller")]
#[command(version = "0.0.1")]
#[command(
    about = "Allows a user to run experiments using defined configs/experiment configs"
)]
#[command(long_about = None)]
struct AppArgs {
    /// The config file storing global configuration
    #[arg(short, long)]
    config: Option<PathBuf>,

    /// The experiment-specific configuration file
    #[arg(short, long)]
    experiment_config: Option<PathBuf>,

    #[arg(short, long, default_value_t = String::from("localhost:50000"))]
    address: String,

    /// The log file storing controller outputs
    #[arg(short, long, default_value=default_log_file().into_os_string())]
    log_file: PathBuf,

    #[arg(short, long, default_value=default_working_directory().into_os_string())]
    working_directory: PathBuf,

    #[arg(short, long, default_value_t = 5)]
    timeout: u64,

    #[command(flatten)]
    pathway: ExecutionPathway,
}

#[derive(Args, Debug)]
#[group(required = false, multiple = false)]
struct ExecutionPathway {
    #[arg(short, long)]
    validate: bool,
    #[arg(short, long)]
    status: bool,
    #[arg(short, long, value_delimiter = ',', value_parser = parse_upload)]
    upload: Vec<(PathBuf, FileType)>,
}
