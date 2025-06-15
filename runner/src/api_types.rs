use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Deserialize)]
pub enum WorkloadType {
    Rke2,
    Native,
}

impl fmt::Display for WorkloadType {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self)
    }
}

#[derive(Deserialize)]
pub struct Job {
    pub job_id: String,
    pub experiment_id: String,
    pub workload: String,
    pub workload_type: WorkloadType,
    pub arguments: Option<String>,
}

impl Job {
    pub fn job_id(&self) -> &String {
        &self.job_id
    }

    pub fn experiment_id(&self) -> &String {
        &self.experiment_id
    }

    pub fn workload(&self) -> &String {
        &self.workload
    }

    pub fn workload_type(&self) -> &WorkloadType {
        &self.workload_type
    }

    // Not sure how I should properly return a reference to an Option<T>
    pub fn arguments(&self) -> &Option<String> {
        &self.arguments
    }
}

#[derive(Serialize)]
pub struct StartJobResponse {
    busy: bool,
    waiting_jobs: u32,
}

impl StartJobResponse {
    pub fn new(busy: bool, waiting_jobs: u32) -> Self {
        Self { busy, waiting_jobs }
    }

    pub fn busy(&self) -> bool {
        self.busy
    }

    pub fn waiting_jobs(&self) -> u32 {
        self.waiting_jobs
    }
}

#[derive(Serialize)]
pub struct ViewJobResponse {
    busy: bool,
    waiting_jobs: u32,
}

impl ViewJobResponse {
    pub fn new(busy: bool, waiting_jobs: u32) -> Self {
        Self { busy, waiting_jobs }
    }

    pub fn busy(&self) -> bool {
        self.busy
    }

    pub fn waiting_jobs(&self) -> u32 {
        self.waiting_jobs
    }
}

#[derive(Deserialize)]
pub struct ExecuteRequest {
    commands: Vec<String>,
}

impl ExecuteRequest {
    pub fn new(commands: Vec<String>) -> Self {
        Self { commands }
    }

    pub fn commands(&self) -> &Vec<String> {
        &self.commands
    }
}

#[derive(Serialize)]
pub struct ExecuteResponse {
    command_responses: Vec<String>,
}

impl ExecuteResponse {
    pub fn new(command_responses: Vec<String>) -> Self {
        Self { command_responses }
    }

    pub fn command_responses(&self) -> &Vec<String> {
        &self.command_responses
    }
}
