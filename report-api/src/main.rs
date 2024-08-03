use axum::{
    routing::{get, post},
    Router,
};

#[tokio::main]
async fn main() {
    let app = Router::new()
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
        .route("/job/{job_id}", get(job_status));

    let listener = tokio::net::TcpListener::bind("localhost:3000")
        .await
        .unwrap();
    let serving = axum::serve(listener, app);
    println!("Now serving at http://localhost:3000!");
    serving.await.unwrap();
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
