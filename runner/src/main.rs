#[macro_use]
extern crate rocket;

pub mod api_types;
pub mod runner;

use api_types::{ExecuteRequest, ExecuteResponse, Job, StartJobResponse, ViewJobResponse};

use runner::Runner;

use std::sync::mpsc;
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

use rocket::{serde::json::Json, State};

#[get("/")]
fn index() -> &'static str {
    concat!(
        "`GET /job` Gets all available jobs and their status\n",
        "`POST /job { <job_request> }` starts a new job from the request\n",
        "`POST /exec { <execute_request> }` Executes a series of commands from the request\n"
    )
}

#[post("/job", data = "<job>")]
fn add_job(
    job: Json<Job>,
    sender: &State<mpsc::Sender<Job>>,
    runner: &State<Runner>,
) -> Json<StartJobResponse> {
    runner.add_job(sender, job.into_inner());

    Json(StartJobResponse::new(
        *runner.busy().lock().unwrap(),
        *runner.waiting_jobs().lock().unwrap(),
    ))
}

#[get("/job")]
fn view_job(runner: &State<Runner>) -> Json<ViewJobResponse> {
    Json(ViewJobResponse::new(
        *runner.busy().lock().unwrap(),
        *runner.waiting_jobs().lock().unwrap(),
    ))
}

#[post("/exec", data = "<execute_request>")]
fn execute(execute_request: Json<ExecuteRequest>, runner: &State<Runner>) -> Json<ExecuteResponse> {
    let responses = runner.run_commands(execute_request.into_inner().commands());
    Json(ExecuteResponse::new(responses))
    // Json(StartJobResponse {
    //     running_now: false,
    //     waiting_jobs: 0,
    // })
}

#[launch]
fn rocket() -> _ {
    let metrics_api_endpoint = String::from("http://localhost:5000");
    let workload_repository_endpoint = String::from("http://localhost:50001");
    let (runner, sender) = Runner::new(metrics_api_endpoint, workload_repository_endpoint);

    // rocket::tokio::spawn(async move {
    //
    // })
    // rocket::tokio::spawn(async {
    //     runner.execute_job
    //
    // }

    rocket::build()
        .mount("/", routes![index, add_job, view_job, execute])
        .manage(sender)
        .manage(runner)

    // .manage(Runner::new())
}
