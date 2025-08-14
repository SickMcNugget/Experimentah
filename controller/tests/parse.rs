use std::{collections::HashMap, path::PathBuf, str::FromStr};

use controller::parse::{
    generate_experiments, Config, Experiment, ExperimentConfig, Exporter,
    ExporterConfig, FileType, Host, HostConfig, RemoteExecution,
    RemoteExecutionConfig, VariationConfig,
};

const VALID_CONFIG: &str = "valid_config.toml";
const VALID_EXPERIMENT_CONFIG: &str = "valid_experiment_config.toml";

fn test_path() -> PathBuf {
    dbg!(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap()).join("tests")
}

#[test]
fn parse_config() {
    dbg!(test_path());
    let config = match Config::from_file(test_path().join(VALID_CONFIG)) {
        Ok(config) => config,
        Err(e) => panic!("A valid config failed to be validated: {e}"),
    };

    if let Err(e) = config.validate() {
        panic!("Unable to validate config: {e}");
    }

    let expected_config = Config {
        hosts: vec![HostConfig {
            name: "runner1".to_string(),
            address: "localhost".to_string(),
        }],
        exporters: vec![
            ExporterConfig {
                name: "test-exporter".to_string(),
                hosts: vec!["runner1".to_owned()],
                command: "sar -o collection.bin".to_string(),
                setup: vec![
                    "dnf install -y sysstat".into(),
                    "apt install -y sysstat".into(),
                ],
            },
            ExporterConfig {
                name: "another-test-exporter".to_string(),
                hosts: vec!["runner1".into()],
                command: "my_collector".into(),
                setup: vec![],
            },
        ],
    };
    assert_eq!(
            config, expected_config,
            "Configs did not match\nActual\n{config:#?}\nExpected\n{expected_config:#?}"
        )
}

#[test]
fn parse_experiment_config() {
    let config_path = test_path().join(VALID_CONFIG);
    let experiment_config_path = test_path().join(VALID_EXPERIMENT_CONFIG);
    let config = match Config::from_file(&config_path) {
        Ok(config) => config,
        Err(e) => panic!("A valid config couldn't be parsed: {e}"),
    };

    if let Err(e) = config.validate() {
        panic!("Unable to validate config: {e}");
    }

    let experiment_config =
        match ExperimentConfig::from_file(&experiment_config_path) {
            Ok(experiment_config) => experiment_config,
            Err(e) => {
                panic!("A valid experiment config couldn't be parsed: {e}")
            }
        };

    if let Err(e) = experiment_config.validate(&config) {
        panic!("Unable to validate experiment config: {e}");
    }

    let hosts = vec!["runner1".into()];
    let expected_experiment_config = ExperimentConfig {
                name: "localhost-experiment".into(),
                description: "Testing the functionality of the software completely using localhost".into(),
                kind: "localhost-result".into(),
                execute: "./scripts/actual-work.sh".into(),
                dependencies: vec![],
                expected_arguments: Some(2),
                arguments: vec!["Argument 1".into(), "Argument 2".into()],
                exporters: vec![],
                hosts: hosts.clone(),
                runs: 1,
                setup: vec![RemoteExecutionConfig {
                        hosts: hosts.clone(),
                        scripts: vec!["./scripts/test-setup.sh".into()]
                    }
                ],
                variations: vec![VariationConfig {
                    name: "base".into(),
                    hosts: vec![],
                    expected_arguments: None,
                    arguments: vec![],
                    exporters: vec![]
                    },
                    VariationConfig {
                        name: "different args".into(),
                        hosts: vec![],
                        expected_arguments: Some(1),
                        arguments: vec!["Argument 1".into()],
                        exporters: vec![]
                    },
                    VariationConfig {
                        name: "with exporter".into(),
                        hosts: vec![],
                        expected_arguments: None,
                        arguments: vec![],
                        exporters: vec!["test-exporter".into()]
                    },
                    VariationConfig {
                        name: "with multiple exporters".into(),
                        hosts: vec![],
                        expected_arguments: None,
                        arguments: vec![],
                        exporters: vec!["test-exporter".into(), "another-test-exporter".into()]
                    }
                ],
                teardown: vec![RemoteExecutionConfig{
                        hosts: hosts.clone(),
                        scripts: vec!["./scripts/test-teardown.sh".into()]
                }],
            };

    assert_eq!(
            experiment_config, expected_experiment_config,
            "Experiment configs did not match\nActual\n{experiment_config:#?}\nExpected\n{expected_experiment_config:#?}"
        )
}

#[test]
fn to_experiments() {
    let config_path = test_path().join(VALID_CONFIG);
    let experiment_config_path = test_path().join(VALID_EXPERIMENT_CONFIG);
    let config = match Config::from_file(&config_path) {
        Ok(config) => config,
        Err(e) => panic!("A valid config couldn't be parsed: {e}"),
    };

    if let Err(e) = config.validate() {
        panic!("Unable to validate config: {e}");
    }

    let experiment_config =
        match ExperimentConfig::from_file(&experiment_config_path) {
            Ok(experiment_config) => experiment_config,
            Err(e) => {
                panic!("A valid experiment config couldn't be parsed: {e}")
            }
        };

    if let Err(e) = experiment_config.validate(&config) {
        panic!("Unable to validate experiment config: {e}");
    }

    let experiments =
        generate_experiments(&config, &experiment_config).unwrap();

    let basepath = std::path::absolute(PathBuf::from("storage")).unwrap();

    let re_host = Host {
        name: "runner1".to_string(),
        address: "localhost".to_string(),
    };

    let scripts: HashMap<&str, PathBuf> = HashMap::from([
        ("setup", basepath.join("setup/test-setup.sh")),
        ("teardown", basepath.join("teardown/test-teardown.sh")),
        ("execute", basepath.join("execute/actual-work.sh")),
    ]);

    let mut remote_executions: HashMap<&str, Vec<RemoteExecution>> =
        HashMap::with_capacity(scripts.len());
    for (stage, script) in scripts.iter() {
        remote_executions.insert(
            stage,
            vec![RemoteExecution {
                hosts: vec![re_host.clone()],
                scripts: vec![script.clone()],
                re_type: FileType::from_str(stage).unwrap(),
            }],
        );
    }

    let exporters = HashMap::from([
        (
            "test-exporter",
            Exporter {
                name: "test-exporter".to_string(),
                hosts: vec![re_host.clone()],
                command: "sar -o collection.bin".to_string(),
                setup: vec![
                    "dnf install -y sysstat".to_string(),
                    "apt install -y sysstat".to_string(),
                ],
            },
        ),
        (
            "another-test-exporter",
            Exporter {
                name: "another-test-exporter".into(),
                hosts: vec![Host {
                    name: "runner1".into(),
                    address: "localhost".into(),
                }],
                command: "my_collector".into(),
                setup: vec![],
            },
        ),
    ]);

    let expected_experiments = vec![
            Experiment {
                id: None,
                name: "localhost-experiment-base".to_string(),
                description: "Testing the functionality of the software completely using localhost".into(),
                kind: "localhost-result".into(),
                setup: remote_executions["setup"].clone(),
                teardown: remote_executions["teardown"].clone(),
                hosts: vec![re_host.clone()],
                execute: remote_executions["execute"].first().unwrap().clone(),
                dependencies: vec![],
                expected_arguments: Some(2),
                arguments: vec!["Argument 1".into(), "Argument 2".into()],
                exporters: vec![]
            },
            Experiment {
                id: None,
                name: "localhost-experiment-different args".to_string(),
                description: "Testing the functionality of the software completely using localhost".into(),
                kind: "localhost-result".into(),
                setup: remote_executions["setup"].clone(),
                teardown: remote_executions["teardown"].clone(),
                hosts: vec![re_host.clone()],
                execute: remote_executions["execute"].first().unwrap().clone(),
                dependencies: vec![],
                arguments: vec!["Argument 1".into()],
                expected_arguments: Some(1),
                exporters: vec![]
            },
            Experiment {
                id: None,
                name: "localhost-experiment-with exporter".to_string(),
                description: "Testing the functionality of the software completely using localhost".into(),
                kind: "localhost-result".into(),
                setup: remote_executions["setup"].clone(),
                teardown: remote_executions["teardown"].clone(),
                hosts: vec![re_host.clone()],
                execute: remote_executions["execute"].first().unwrap().clone(),
                dependencies: vec![],
                expected_arguments: Some(2),
                arguments: vec!["Argument 1".into(), "Argument 2".into()],
                exporters: vec![exporters["test-exporter"].clone()]
            },
            Experiment {
                id: None,
                name: "localhost-experiment-with multiple exporters".to_string(),
                description: "Testing the functionality of the software completely using localhost".into(),
                kind: "localhost-result".into(),
                setup: remote_executions["setup"].clone(),
                teardown: remote_executions["teardown"].clone(),
                hosts: vec![re_host.clone()],
                execute:  remote_executions["execute"].first().unwrap().clone(),
                dependencies: vec![],
                expected_arguments: Some(2),
                arguments: vec!["Argument 1".into(), "Argument 2".into()],
                exporters: vec![exporters["test-exporter"].clone(), exporters["another-test-exporter"].clone()]
            },
        ];

    let expected_runs = 1;
    assert_eq!(experiments.0, expected_runs,
            "Experiment runs did not match\nActual\n{:#?}\nExpected\n{expected_runs:#?}",
            experiments.0
        );

    assert_eq!(experiments.1, expected_experiments,
            "Experiments did not match\nActual\n{:#?}\nExpected\n{expected_experiments:#?}",
            experiments.1
        )
}
