use serde::{Deserialize, Serialize};
use std::fmt;

#[cfg_attr(test, derive(Serialize))]
#[derive(Deserialize)]
pub struct Experiment {
    // Fields aren't pub since we want to ensure Experiments
    // undergo validation on creation
    id: String,
    // Absolute file path to shell script for execution.
    workload: String,
    arguments: Option<String>,
}

// TODO: Add error checking for workload
impl Experiment {
    pub fn new(id: String, workload: String, arguments: Option<String>) {}
    pub fn get_id(&self) -> &String {
        &self.id
    }
    pub fn get_workload(&self) -> &String {
        &self.workload
    }
    pub fn get_arguments(&self) -> &Option<String> {
        &self.arguments
    }
    pub fn set_id(&mut self, id: String) {
        self.id = id;
    }
    pub fn set_workload(&mut self, workload: String) {
        self.workload = workload;
    }
    pub fn set_arguments(&mut self, arguments: Option<String>) {
        self.arguments = arguments;
    }
}

#[cfg_attr(test, derive(Deserialize))]
#[derive(Serialize)]
pub struct ExperimentResponse {
    pub busy: bool,
    pub waiting_experiments: u32,
}

impl ExperimentResponse {
    pub fn new(busy: bool, waiting_experiments: u32) -> Self {
        Self {
            busy,
            waiting_experiments,
        }
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
