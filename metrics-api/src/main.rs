use std::{
    collections::HashMap,
    env,
    ffi::OsString,
    fs::read_dir,
    io::{self, ErrorKind},
    net::SocketAddr,
    path::PathBuf,
};

use actix_web::{get, post, web, App, HttpResponse, HttpServer, Responder};
use metrics_api::models::*;

// use axum::{
//     http::StatusCode,
//     routing::{get, post},
//     Router,
// };

use diesel::{prelude::*, query_dsl::methods::SelectDsl};

use dotenvy::dotenv;
use reqwest;

// use tower_http::services::ServeDir;
fn connect_to_postgres() -> PgConnection {
    let database_url = env::var("DATABASE_URL").expect("DATABASE_URL must be set");
    PgConnection::establish(&database_url)
        .unwrap_or_else(|_| panic!("Error connecting to {}", database_url))
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    use metrics_api::schema::experiments::dsl::*;

    dotenv().ok();
    let conn = &mut connect_to_postgres();

    let results = experiments
        .limit(5)
        .select(Experiment::as_select())
        .load(conn)
        .expect("Error loading experiments");
    println!("Displaying {} experiments", results.len());

    HttpServer::new(|| App::new().service(status))
        .bind(("127.0.0.1", 50000))?
        .run()
        .await
    // .service(prometheus_reset)
    // .service(prometheus_status)
    // .service(prometheus_configure)
    // .service(prometheus_get)
}
//
// async fn serve(name: &str, app: Router, port: u16) {
//     let addr = SocketAddr::from(([127, 0, 0, 1], port));
//     let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
//     println!("{name} listening on {}", listener.local_addr().unwrap());
//     axum::serve(listener, app).await.unwrap();
// }
//
// fn get_root() -> io::Result<PathBuf> {
//     let path = env::current_dir()?;
//     let mut path_ancestors = path.as_path().ancestors();
//
//     while let Some(p) = path_ancestors.next() {
//         let is_root = read_dir(p)?
//             .into_iter()
//             .any(|p| p.unwrap().file_name() == OsString::from("Cargo.toml"));
//         if is_root {
//             return Ok(PathBuf::from(p));
//         }
//     }
//
//     Err(io::Error::new(
//         ErrorKind::NotFound,
//         "Ran out of places to find Cargo.toml",
//     ))
// }
//
// fn workload_repo() -> Router {
//     let path = get_root().unwrap();
//     println!("{:?}", path.join("workload_repo"));
//     Router::new().nest_service("/workload_repo", get(())) // ServeDir::new(path.join("workload_repo")))
// }
//
//
// async fn root() -> &'static str {
//     concat!(
//         "# Displays all routes\n",
//         "GET /\n",
//         "# Checks the status of all servers (Prometheus, etc., etc.)\n",
//         "GET /status\n\n",
//         "# Returns the status of prometheus (healthy, ready, error)\n",
//         "GET /prometheus\n",
//         "# Registers exporters in prometheus to begin collecting data from them\n",
//         "POST /prometheus/configure\n",
//         "# Deletes all exporter configuration in prometheus, preventing further collection\n",
//         "POST /prometheus/reset\n",
//         "# Queries data from prometheus and returns it as JSON\n",
//         "GET /prometheus/<exporter_id>\n\n",
//         "# Registers an experiment in the database and prepares to collect data from it\n",
//         "POST /experiment\n",
//         "# Returns a completed experiment and all its jobs as JSON\n",
//         "GET /experiment/<experiment_id>\n\n",
//         "# Registers a job in the database and prepares to collect data from it\n",
//         "POST /job\n",
//         "# Returns a completed job as JSON\n",
//         "GET /job/<job_id>\n",
//         "# Updates a job in the database as started\n",
//         "POST /job/start\n",
//         "# Updates a job in the database as completed\n",
//         "POST /job/complete\n",
//         "# Updates a job in the database as failed\n",
//         "POST /job/fail\n",
//     )
// }
//
#[get("/status")]
async fn status() -> impl Responder {
    let client = reqwest::Client::new();

    // let mongo_host = env::var("MONGO_HOST")?;
    // let mongo_port = env::var("MONGO_PORT")?;
    // let mongo_username = env::var("MONGO_USERNAME")?;
    // let mongo_password = env::var("MONGO_PASSWORD")?;
    // let mongo_authentication_server = env::var("MONGO_AUTHENTICATION_SERVER")?;

    let prometheus_host = env::var("PROMETHEUS_HOST");
    let prometheus_port = env::var("PROMETHEUS_PORT");
    let prometheus_username = env::var("PROMETHEUS_USERNAME");
    let prometheus_password = env::var("PROMETHEUS_PASSWORD");
    let prometheus_authentication_server = env::var("PROMETHEUS_AUTHENTICATION_SERVER");

    let prometheus_request = client.get("http://PROMETHEUS_URL/-/healthy").send();
    let mongo_request = "yes";

    "yes"
}

// #[get("/prometheus")]
// async fn prometheus_status() {}
//
// #[post("/prometheus/configure")]
// async fn prometheus_configure() {}
//
// #[post("/prometheus/reset")]
// async fn prometheus_reset() {}
//
// #[get("/prometheus/{exporter_id}")]
// async fn prometheus_get(exporter_id: web::Path<String>) -> impl Responder {
//     "Ligma"
// }
//
// #[post("/experiment")]
// async fn experiment_create() {
//     // POSTGRES Create experiment
//     //
// }
// // async fn experiment_setup() {}
// // async fn experiment_teardown() {}
// #[get("/experiment/{experiment_id}")]
// async fn experiment_get(experiment_id: web::Path<String>) {}
//
// #[post("/job")]
// async fn job_create() {}
// #[post("/job/start")]
// async fn job_start() {}
// #[post("/job/complete")]
// async fn job_complete() {}
// #[post("/job/fail")]
// async fn job_failed() {}
// #[get("/job/{job_id}")]
// async fn job_status(job_id: web::Path<String>) {}
