use std::{
    env,
    ffi::OsString,
    fs::read_dir,
    io::{self, ErrorKind},
    net::SocketAddr,
    path::PathBuf,
};

use axum::{
    routing::{get, post},
    Router,
};

use tower_http::services::ServeDir;

#[tokio::main]
async fn main() {
    tokio::join!(
        serve("Metrics API", report_api(), 3000),
        serve("Workload Repo", workload_repo(), 3001)
    );
}

async fn serve(name: &str, app: Router, port: u16) {
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    println!("{name} listening on {}", listener.local_addr().unwrap());
    axum::serve(listener, app).await.unwrap();
}

fn get_root() -> io::Result<PathBuf> {
    let path = env::current_dir()?;
    let mut path_ancestors = path.as_path().ancestors();

    while let Some(p) = path_ancestors.next() {
        let is_root = read_dir(p)?
            .into_iter()
            .any(|p| p.unwrap().file_name() == OsString::from("Cargo.toml"));
        if is_root {
            return Ok(PathBuf::from(p));
        }
    }

    Err(io::Error::new(
        ErrorKind::NotFound,
        "Ran out of places to find Cargo.toml",
    ))
}

fn workload_repo() -> Router {
    let path = get_root().unwrap();
    println!("{:?}", path.join("workload_repo"));
    Router::new().nest_service("/workload_repo", ServeDir::new(path.join("workload_repo")))
}

fn report_api() -> Router {
    Router::new()
        .route("/", get(root))
        .route("/status", get(status))
        .route("/prometheus", get(prometheus_status))
        .route("/prometheus/configure", post(prometheus_configure))
        .route("/prometheus/reset", post(prometheus_reset))
        .route("/prometheus/{exporter_id}", get(prometheus_get))
        .route("/experiment", post(experiment_create))
        // .route("/experiment/{exp_id}/setup", post(experiment_setup))
        // .route("/experiment/{exp_id}/teardown", post(experiment_teardown))
        .route("/experiment/:id", get(experiment_get))
        .route("/job", post(job_create))
        .route("/job/{job_id}/start", post(job_start))
        .route("/job/{job_id}/complete", post(job_complete))
        .route("/job/{job_id}/fail", post(job_failed))
        .route("/job/{job_id}", get(job_status))
}

async fn root() -> &'static str {
    concat!(
        "# Displays all routes\n",
        "GET /\n",
        "# Checks the status of all servers (Prometheus, etc., etc.)\n",
        "GET /status\n\n",
        "# Returns the status of prometheus (healthy, ready, error)\n",
        "GET /prometheus\n",
        "# Registers exporters in prometheus to begin collecting data from them\n",
        "POST /prometheus/configure\n",
        "# Deletes all exporter configuration in prometheus, preventing further collection\n",
        "POST /prometheus/reset\n",
        "# Queries data from prometheus and returns it as JSON\n",
        "GET /prometheus/<exporter_id>\n\n",
        "# Registers an experiment in the database and prepares to collect data from it\n",
        "POST /experiment\n",
        "# Returns a completed experiment and all its jobs as JSON\n",
        "GET /experiment/<experiment_id>\n\n",
        "# Registers a job in the database and prepares to collect data from it\n",
        "POST /job\n",
        "# Returns a completed job as JSON\n",
        "GET /job/<job_id>\n",
        "# Updates a job in the database as started\n",
        "POST /job/start\n",
        "# Updates a job in the database as completed\n",
        "POST /job/complete\n",
        "# Updates a job in the database as failed\n",
        "POST /job/fail\n",
    )
}
async fn status() {}
async fn prometheus_status() {}
async fn prometheus_configure() {}
async fn prometheus_reset() {}
async fn prometheus_get() {}
async fn experiment_create() {}
// async fn experiment_setup() {}
// async fn experiment_teardown() {}
async fn experiment_get() {}
async fn job_create() {}
async fn job_start() {}
async fn job_complete() {}
async fn job_failed() {}
async fn job_status() {}
