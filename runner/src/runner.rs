use super::api_types::Job;
use regex::Regex;
use reqwest::Client;
use serde::Serialize;
use std::collections::HashMap;
use std::fmt;
use std::process::Command;
use std::result;
use std::string;
use std::{
    sync::{
        mpsc::{self, Receiver, Sender},
        Mutex,
    },
    time::{SystemTime, UNIX_EPOCH},
};

pub type Result<T> = result::Result<T, RunnerError>;

struct RunnerError {
    details: String,
}

impl RunnerError {
    fn new(details: String) -> Self {
        RunnerError { details }
    }
}

impl fmt::Display for RunnerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.details.as_str())
    }
}

impl fmt::Debug for RunnerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.details.as_str())
    }
}

impl From<reqwest::Error> for RunnerError {
    fn from(value: reqwest::Error) -> Self {
        RunnerError {
            details: value.without_url().to_string(),
        }
    }
}

impl From<string::FromUtf8Error> for RunnerError {
    fn from(value: string::FromUtf8Error) -> Self {
        RunnerError {
            details: value.to_string(),
        }
    }
}

impl From<&str> for RunnerError {
    fn from(value: &str) -> Self {
        RunnerError {
            details: value.to_string(),
        }
    }
}

pub struct Runner {
    job_queue: Mutex<Receiver<Job>>,
    busy: Mutex<bool>,
    waiting_jobs: Mutex<u32>,
    running: bool,
    client: reqwest::Client,
    brain_endpoint: String,
    // workload_repository_endpoint: String,
    job_regex: Regex,
}

impl Runner {
    pub fn new(brain_endpoint: String) -> (Self, Sender<Job>) {
        let (sender, receiver) = mpsc::channel();
        let runner = Runner {
            job_queue: Mutex::new(receiver),
            busy: Mutex::new(false),
            waiting_jobs: Mutex::new(0),
            running: true,
            client: Client::new(),
            brain_endpoint,
            job_regex: Regex::new(r"%%%%%\nworkload_output:@@@@@(?<workload_output>.+?)@@@@@\nstart_time_unix:@@@@@(?<start_time_unix>\d+)@@@@@\nend_time_unix:@@@@@(?<end_time_unix>\d+)@@@@@\n%%%%%").unwrap(),
        };

        (runner, sender)
    }

    pub fn busy(&self) -> &Mutex<bool> {
        &self.busy
    }

    pub fn waiting_jobs(&self) -> &Mutex<u32> {
        &self.waiting_jobs
    }

    fn get_time(&self) -> String {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("Time went backwards")
            .as_millis()
            .to_string()
    }

    fn build_exec_command(&self, job: &Job) -> Command {
        let mut command_args = vec![format!(
            "./resources/jobs/{}/{}.sh",
            job.workload_type, job.workload
        )];

        if let Some(args) = &job.arguments {
            command_args.push(args.to_string())
        };

        let mut command = Command::new("/bin/sh");
        command.arg("-c").args(command_args);
        command
    }

    pub fn add_job(&self, sender: &Sender<Job>, job: Job) {
        sender
            .send(job)
            .expect("Unable to send job to runner queue");
        *self.waiting_jobs.lock().unwrap() += 1;
    }

    pub fn run_commands(&self, commands: &Vec<String>) -> Vec<String> {
        let mut responses: Vec<String> = Vec::new();
        for command in commands {
            let result = Command::new("bash").arg("-c").arg(command).output();
            let output = match result {
                Ok(output) => match String::from_utf8(output.stderr) {
                    Ok(string) => string,
                    Err(e) => e.to_string(),
                },
                Err(e) => e.to_string(),
            };
            responses.push(output);
        }
        return responses;
    }

    // async fn run_loop(&self) {
    //     while let Ok(job) = self
    //         .job_queue
    //         .lock()
    //         .expect("Unable to acquire mutex for job queue")
    //         .recv()
    //     {
    //         {
    //             *self.busy.lock().expect("Unable to lock busy bool") = true
    //         };
    //
    //         match self.execute_job(job) {
    //             Ok(_) => {}
    //             Err(_) => {}
    //         }
    //     }
    // }

    async fn brain_request<T>(
        &self,
        request_type: &str,
        job: &Job,
        body: T,
    ) -> Result<reqwest::Response>
    where
        T: Serialize,
    {
        let response = self
            .client
            .post(format!(
                "http://{}/experiment/{}/job/{}/{}",
                self.brain_endpoint, job.experiment_id, job.job_id, request_type
            ))
            .json(&body)
            .send()
            .await?;

        if !response.status().is_success() {
            return Err(RunnerError::new(format!(
                "Unable to contact metrics api: {}\nreason: {}",
                self.brain_endpoint,
                response.text().await?
            )));
        }

        return Ok(response);
    }

    async fn execute_job(&self, job: &Job) -> Result<()> {
        println!(
            "Executing '{}' - execute '{}' on '{}'",
            job.experiment_id, job.workload, job.workload_type
        );

        println!("[LOG] Reporting job start");
        let map = HashMap::from([("runnerTimestampMs", self.get_time())]);
        self.brain_request("start", job, map).await?;

        let mut command = self.build_exec_command(job);

        println!("[LOG] Executing command: {:?}", command);

        let result = match command.output() {
            Ok(result) => result,
            Err(e) => {
                let body = HashMap::from([
                    ("reason", e.to_string()),
                    ("runnerTimestampMs", self.get_time()),
                ]);
                let response = self.brain_request("error", job, body).await?;

                // If we got this far, we have an error
                return Err(RunnerError::new(format!(
                    "Successfully logged failure - {}",
                    response.text().await?
                )));
            }
        };

        let parsed_result = self.parse_job_output(result.stdout)?;

        Ok(())
    }

    fn parse_job_output(&self, bytes: Vec<u8>) -> Result<HashMap<&str, String>> {
        let output: String = String::from_utf8(bytes)?.trim().into();

        let caps = self
            .job_regex
            .captures(output.as_str())
            .ok_or("Unable to parse job output")?;

        // output
        Ok(HashMap::from([
            ("runnerTimestampMs", self.get_time()),
            ("resultsRaw", caps["workload_output"].into()),
            ("processStartTimeMs", caps["start_time_unix"].into()),
            ("processEndTimeMs", caps["end_time_unix"].into()),
        ]))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn job_parsing() {
        let bytes: Vec<u8> = "%%%%%\nworkload_output:@@@@@I did it dad!@@@@@\nstart_time_unix:@@@@@1721742181@@@@@\nend_time_unix:@@@@@1721742182@@@@@\n%%%%%".as_bytes().to_vec();
        let expected = HashMap::from([
            ("resultsRaw", "I did it dad!"),
            ("processStartTimeMs", "1721742181"),
            ("processEndTimeMs", "1721742182"),
        ]);

        let (runner, _) = Runner::new("127.0.0.1:50000".into(), "127.0.0.1:50001".into());
        let actual = runner.parse_job_output(bytes).unwrap();

        assert_eq!(expected["resultsRaw"], actual["resultsRaw"]);
        assert_eq!(expected["processStartTimeMs"], actual["processStartTimeMs"]);
        assert_eq!(expected["processEndTimeMs"], actual["processEndTimeMs"]);
    }
}
