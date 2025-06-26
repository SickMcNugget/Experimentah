use serde::{Deserialize, Serialize};
use std::{fmt, fs};

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
    pub fn new(
        id: String,
        workload: String,
        arguments: Option<String>,
    ) -> Self {
        // Can't find a way to initialise just with the setters.
        // Use placeholder workaround
        // TODO: Review
        let mut p = Experiment {
            id: String::new(),
            workload: String::new(),
            arguments: None,
        };
        p.set_id(id);
        p.set_workload(workload);
        p.set_arguments(arguments);
        p
    }
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
    // Validate that the workload file actually exists
    pub fn set_workload(&mut self, workload: String) {
        // Quit if the shell script doesn't exist for a run workload.
        // Alternatively, could set to a default or None and have the user
        // check it, but this seems safer for the base case and seems easiest
        // to handle consistently (setting invalid workload on currently valid Experiment etc.).
        match fs::exists(&workload) {
            Ok(exists) => {
                if exists {
                    println!("Valid workload {}", &workload);
                    self.workload = workload;
                } else {
                    panic!("Couldn't find workload script {} - must exist before running experiments.", &workload);
                }
            }
            Err(e) => {
                panic!(
                    "Can't confirm file existence for {} with error {}.",
                    &workload, e
                );
            }
        }
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

#[cfg_attr(test, derive(Serialize))]
#[derive(Deserialize)]
pub struct ExecuteRequest {
    pub commands: Vec<String>,
}

#[cfg_attr(test, derive(Deserialize))]
#[derive(Serialize)]
pub struct ExecuteResponse {
    pub command_responses: Vec<String>,
}
