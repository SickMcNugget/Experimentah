pub mod common;

use crate::common::{
    configs_path, read_in_config, read_in_default_config,
    read_in_default_configs, read_in_experiment_config, test_path,
    to_string_side_by_side,
};
use std::{
    collections::HashMap,
    path::PathBuf,
    str::{self, FromStr},
};

use controller::parse::{
    generate_experiments, CommandSource, Config, Experiment, ExperimentConfig,
    Exporter, ExporterConfig, FileType, Host, HostConfig, RemoteExecution,
    RemoteExecutionConfig, VariationConfig,
};
use serde::de::DeserializeOwned;

use toml::{Table, Value};

macro_rules! build_table {
    ($($const:expr),*) => {{
        let mut table = Table::new();
        $(
        let parsed: Value = toml::de::from_str($const).unwrap();

        for (key, value) in parsed.as_table().unwrap() {
            table.insert(key.clone(), value.clone());
        }
        )*
        table
    }}
}

// This allows us to test different combinations of config fields,
// asserting whether those fields should succeed or panic.
// true means success, false means panic
#[derive(Clone, Debug)]
struct ConfigTestCase {
    toml: Table,
    should_parse: bool,
    should_validate: bool,
    expected_missing: bool,
}

impl ConfigTestCase {
    pub fn new(toml: Table, should_parse: bool, should_validate: bool) -> Self {
        Self {
            toml,
            should_parse,
            should_validate,
            expected_missing: false,
        }
    }
}

type ConfigTestCases = Vec<ConfigTestCase>;

const CONFIG_DEFAULT_HOSTS: &str = r#"[[hosts]]
name = "test-runner"
address = "user@myserver.internal""#;

fn parse_and_validate_config(
    test_case: &ConfigTestCase,
    toml_str: &str,
) -> Option<Config> {
    let config = match Config::from_str(&toml_str) {
        Ok(config) => {
            if !test_case.should_parse {
                panic!("An invalid config parsed successfully\n{test_case:?}");
            }

            config
        }
        Err(e) => {
            if test_case.should_parse {
                panic!("A valid config failed to be parsed: {e}")
            }
            println!("An invalid config failed to be parsed: {e}");
            return None;
        }
    };

    match config.validate() {
        Ok(()) => {
            if !test_case.should_validate {
                dbg!(&test_case);
                panic!("An invalid config was validated successfully");
            }
        }
        Err(e) => {
            if test_case.should_validate {
                panic!("Failed to validate a valid config: {e}")
            }
            println!("An invalid config failed to be validated: {e}")
        }
    }

    Some(config)
}

fn config_parse_generic<F>(test_cases: ConfigTestCases, builder_fn: F)
where
    F: Fn(&ConfigTestCase) -> Config,
{
    for test_case in test_cases.iter() {
        let toml_str = toml::to_string_pretty(&test_case.toml).unwrap();

        let config = match parse_and_validate_config(test_case, &toml_str) {
            Some(config) => config,
            None => continue,
        };

        // At this point, we expect that our key is missing (because it isn't present as part of
        // the test), so we just skip to the next test, as we're happy here.
        if test_case.expected_missing {
            continue;
        }

        let expected_config = builder_fn(test_case);

        assert!(
            config == expected_config,
            "{}",
            to_string_side_by_side("Config", &config, &expected_config, 63)
        );
    }
}

#[test]
fn config_parse_hosts() {
    let start_table = Table::new();

    let tests = [
        (None, true, true),
        (Some(vec![]), true, true),
        (
            Some(vec![HostConfig::new(
                "test-runner",
                "user@myserver.internal",
            )]),
            true,
            true,
        ),
        (
            Some(vec![
                HostConfig::new("test-runner", "user@myserver.internal"),
                HostConfig::new("runner2", "root@anotherserver.internal"),
            ]),
            true,
            true,
        ),
        (
            Some(vec![HostConfig::new("test-runner", "localhost")]),
            true,
            false,
        ),
        (
            Some(vec![HostConfig::new("test-runner", "myserver.internal")]),
            true,
            false,
        ),
        (
            Some(vec![HostConfig::new("", "user@myserver.internal")]),
            true,
            false,
        ),
        (Some(vec![HostConfig::new("test-runner", "")]), true, false),
    ];

    let test_cases = map_tests_config(&tests, start_table, |table, hosts| {
        let hosts: Vec<Value> = hosts
            .into_iter()
            .map(|host| {
                Value::try_from(host)
                    .expect("Failed to serialize host to table")
            })
            .collect();
        table.insert("hosts".to_string(), Value::Array(hosts));
    });

    config_parse_generic(test_cases, |test_case| {
        let hosts: Vec<HostConfig> =
            deserialize_value(&test_case.toml["hosts"]);

        Config {
            hosts,
            exporters: vec![],
        }
    });
}

#[test]
fn config_parse_exporters() {
    let start_table = build_table!(CONFIG_DEFAULT_HOSTS);

    let tests = [
        (Some(vec![]), true, true),
        (
            Some(vec![ExporterConfig::new(
                "test-exporter",
                vec!["test-runner"],
                CommandSource::from_command(
                    "dnf install -y sysstat; apt install -y sysstat",
                ),
            )
            .with_setup(CommandSource::from_script("sarexporter.sh"))
            .clone()]),
            true,
            true,
        ),
        (
            Some(vec![
                ExporterConfig::new(
                    "test-exporter",
                    vec!["test-runner"],
                    CommandSource::from_command(
                        "dnf install -y sysstat; apt install -y sysstat",
                    ),
                )
                .with_setup(CommandSource::from_script("sarexporter.sh"))
                .clone(),
                ExporterConfig::new(
                    "test-exporter-2",
                    vec!["localhost"],
                    CommandSource::from_script("my_collector.sh"),
                )
                .with_setup(CommandSource::from_command(
                    "apt install my_collector_package",
                ))
                .clone(),
            ]),
            true,
            true,
        ),
        (
            Some(vec![ExporterConfig::new(
                "test-exporter",
                vec!["localhost"],
                CommandSource::from_command("sar -o sar_output.bin 1 10"),
            )
            .with_setup(CommandSource::from_command(
                "dnf install -y sysstat; apt install -y sysstat",
            ))
            .clone()]),
            true,
            true,
        ),
    ];

    let test_cases =
        map_tests_config(&tests, start_table, |table, exporters| {
            let exporters: Vec<Value> = exporters
                .into_iter()
                .map(|exporter| {
                    Value::try_from(exporter)
                        .expect("Failed to serialize exporter to table")
                })
                .collect();
            table.insert("exporters".to_string(), Value::Array(exporters));
        });

    config_parse_generic(test_cases, |test_case| {
        let exporters: Vec<ExporterConfig> =
            deserialize_value(&test_case.toml["exporters"]);

        let hosts: Vec<HostConfig> = deserialize_constant(CONFIG_DEFAULT_HOSTS);
        dbg!(&test_case);

        Config { hosts, exporters }
    });
}

#[derive(Clone, Debug)]
struct ExperimentConfigTestCase {
    toml: Table,
    should_parse: bool,
    should_validate: bool,
    expected_missing: bool,
}

impl ExperimentConfigTestCase {
    pub fn new(toml: Table, should_parse: bool, should_validate: bool) -> Self {
        Self {
            toml,
            should_parse,
            should_validate,
            expected_missing: false,
        }
    }
}
// type ExperimentConfigTestCases<S> = Vec<ExperimentConfigTestCase<S>>;
type ExperimentConfigTestCases = Vec<ExperimentConfigTestCase>;

const EXPERIMENT_CONFIG_DEFAULT_NAME: &str = r#"name = "default name""#;
const EXPERIMENT_CONFIG_DEFAULT_DESCRIPTION: &str =
    r#"description = "default description""#;
const EXPERIMENT_CONFIG_DEFAULT_SCRIPT: &str =
    r#"script = "default_script.sh""#;
const EXPERIMENT_CONFIG_DEFAULT_HOSTS: &str = r#"hosts = [ "test-runner" ]"#;

fn deserialize_constant<T: DeserializeOwned>(constant: &'static str) -> T {
    // Handle the case where our toml starts with a [[hosts]] header
    let key = if constant.starts_with("[[") {
        &constant[2..constant.find("]]").expect("Invalid table header in toml")]
    } else {
        constant.split(" = ").collect::<Vec<&str>>()[0]
    };

    let table: Table = deserialize_value(
        &toml::de::from_str(constant).expect("Constant was not valid TOML"),
    );

    let value: &Value = table
        .get(key)
        .expect(&format!("Key: {key} not found in table"));
    deserialize_value(value)
}

fn deserialize_value<T: DeserializeOwned>(value: &Value) -> T {
    value
        .clone()
        .try_into()
        .expect("Failed to convert value to type")
}

fn map_tests<T, F>(
    tests: &[(Option<T>, bool, bool)],
    start_table: Table,
    mut convert_fn: F,
) -> ExperimentConfigTestCases
where
    F: FnMut(&mut Table, &T),
{
    tests
        .iter()
        .map(|(val, should_parse, should_validate)| {
            let mut table = start_table.clone();

            if let Some(val) = val {
                convert_fn(&mut table, val);
            }

            let mut test_case = ExperimentConfigTestCase::new(
                table,
                *should_parse,
                *should_validate,
            );

            if val.is_none() {
                test_case.expected_missing = true;
            }

            test_case
        })
        .collect()
}

fn map_tests_config<T, F>(
    tests: &[(Option<T>, bool, bool)],
    start_table: Table,
    mut convert_fn: F,
) -> ConfigTestCases
where
    F: FnMut(&mut Table, &T),
{
    tests
        .iter()
        .map(|(val, should_parse, should_validate)| {
            let mut table = start_table.clone();

            if let Some(val) = val {
                convert_fn(&mut table, val);
            }

            let mut test_case =
                ConfigTestCase::new(table, *should_parse, *should_validate);

            if val.is_none() {
                test_case.expected_missing = true;
            }

            test_case
        })
        .collect()
}

fn parse_and_validate_experiment_config(
    test_case: &ExperimentConfigTestCase,
    toml_str: &str,
    config: &Config,
) -> Option<ExperimentConfig> {
    let experiment_config = match ExperimentConfig::from_str(&toml_str) {
        Ok(experiment_config) => {
            if !test_case.should_parse {
                panic!(
                    "An invalid experiment parsed successfully\n{test_case:?}"
                );
            }

            experiment_config
        }
        Err(e) => {
            if test_case.should_parse {
                panic!("A valid experiment config failed to be parsed: {e}")
            }
            println!("An invalid experiment config failed to be parsed: {e}");
            return None;
        }
    };

    match experiment_config.validate(config) {
        Ok(()) => {
            if !test_case.should_validate {
                dbg!(&test_case);
                panic!(
                    "An invalid experiment config was validated successfully"
                );
            }
        }
        Err(e) => {
            if test_case.should_validate {
                dbg!(&test_case);
                panic!("Failed to validate a valid experiment config: {e}")
            }
            println!("An invalid experiment config failed to be validated: {e}")
        }
    }
    Some(experiment_config)
}

fn experiment_config_parse_generic<F>(
    test_cases: ExperimentConfigTestCases,
    builder_fn: F,
) where
    F: Fn(&ExperimentConfigTestCase) -> ExperimentConfig,
{
    let config = read_in_default_config();
    for test_case in test_cases.iter() {
        let toml_str = toml::to_string_pretty(&test_case.toml).unwrap();

        let experiment_config = match parse_and_validate_experiment_config(
            test_case, &toml_str, &config,
        ) {
            Some(experiment_config) => experiment_config,
            None => continue,
        };

        // At this point, we expect that our key is missing (because it isn't present as part of
        // the test), so we just skip to the next test, as we're happy here.
        if test_case.expected_missing {
            continue;
        }

        let expected_experiment_config = builder_fn(test_case);

        assert!(
            experiment_config == expected_experiment_config,
            "{}",
            to_string_side_by_side(
                "Experiment Config",
                &experiment_config,
                &expected_experiment_config,
                63
            )
        );
    }
}

#[test]
fn experiment_config_parse_name() {
    let start_table = build_table!(
        EXPERIMENT_CONFIG_DEFAULT_DESCRIPTION,
        EXPERIMENT_CONFIG_DEFAULT_SCRIPT,
        EXPERIMENT_CONFIG_DEFAULT_HOSTS
    );

    let tests = [
        (None, false, false),
        (Some(""), true, false),
        (Some("What the actual fudge"), true, true),
    ];

    let test_cases = map_tests(&tests, start_table, |table, name| {
        table.insert("name".to_string(), Value::String(name.to_string()));
    });

    experiment_config_parse_generic(test_cases, |test_case| {
        // let name = val_to_str(&test_case.toml["name"]);
        let name: String = deserialize_value(&test_case.toml["name"]);
        let description: String =
            deserialize_constant(EXPERIMENT_CONFIG_DEFAULT_DESCRIPTION);
        // val_to_str(&constant_to_val(EXPERIMENT_CONFIG_DEFAULT_DESCRIPTION));
        let script: PathBuf =
            deserialize_constant(EXPERIMENT_CONFIG_DEFAULT_SCRIPT);
        // val_to_str(&constant_to_val(EXPERIMENT_CONFIG_DEFAULT_SCRIPT));
        let hosts: Vec<String> =
            deserialize_constant(EXPERIMENT_CONFIG_DEFAULT_HOSTS);
        // val_to_str_array(&constant_to_val(EXPERIMENT_CONFIG_DEFAULT_HOSTS));

        ExperimentConfig::new(name, description, script, &hosts)
    });
}

#[test]
fn experiment_config_parse_description() {
    let start_table = build_table!(
        EXPERIMENT_CONFIG_DEFAULT_NAME,
        EXPERIMENT_CONFIG_DEFAULT_SCRIPT,
        EXPERIMENT_CONFIG_DEFAULT_HOSTS
    );

    let tests = [
            (None, false, false),
            (Some(""), true, false),
            (Some("This is the absolute craziest description I could come up with"), true, true)
        ];

    let test_cases = map_tests(&tests, start_table, |table, description| {
        table.insert(
            "description".to_string(),
            Value::String(description.to_string()),
        );
    });

    experiment_config_parse_generic(test_cases, |test_case| {
        let description: String =
            deserialize_value(&test_case.toml["description"]);

        let name: String = deserialize_constant(EXPERIMENT_CONFIG_DEFAULT_NAME);
        let script: PathBuf =
            deserialize_constant(EXPERIMENT_CONFIG_DEFAULT_SCRIPT);
        let hosts: Vec<String> =
            deserialize_constant(EXPERIMENT_CONFIG_DEFAULT_HOSTS);

        ExperimentConfig::new(name, description, script, &hosts)
    });
}

#[test]
fn experiment_config_parse_script() {
    let start_table = build_table!(
        EXPERIMENT_CONFIG_DEFAULT_NAME,
        EXPERIMENT_CONFIG_DEFAULT_DESCRIPTION,
        EXPERIMENT_CONFIG_DEFAULT_HOSTS
    );

    let tests = [
        (None, false, false),
        (Some(""), true, false),
        (Some("generated_file.sh"), true, true),
    ];

    let test_cases = map_tests(&tests, start_table, |table, script| {
        table.insert("script".to_string(), Value::String(script.to_string()));
    });

    experiment_config_parse_generic(test_cases, |test_case| {
        let script: PathBuf = deserialize_value(&test_case.toml["script"]);

        let name: String = deserialize_constant(EXPERIMENT_CONFIG_DEFAULT_NAME);
        let description: String =
            deserialize_constant(EXPERIMENT_CONFIG_DEFAULT_DESCRIPTION);
        let hosts: Vec<String> =
            deserialize_constant(EXPERIMENT_CONFIG_DEFAULT_HOSTS);

        ExperimentConfig::new(name, description, script, &hosts)
    });
}

#[test]
fn experiment_config_parse_hosts() {
    let start_table = build_table!(
        EXPERIMENT_CONFIG_DEFAULT_NAME,
        EXPERIMENT_CONFIG_DEFAULT_DESCRIPTION,
        EXPERIMENT_CONFIG_DEFAULT_SCRIPT
    );

    let tests = [
        (None, false, false),
        (Some(vec![""]), true, false),
        (Some(vec!["test-runner"]), true, true),
        (Some(vec!["localhost"]), true, true),
        (Some(vec!["test-runner", "localhost"]), true, true),
        (Some(vec!["nonexistanthost"]), true, false),
        (Some(vec!["test-runner", "nonexistanthost"]), true, false),
    ];

    let test_cases = map_tests(&tests, start_table, |table, hosts| {
        let hosts: Vec<Value> = hosts
            .into_iter()
            .map(|host| Value::String(host.to_string()))
            .collect();
        table.insert("hosts".to_string(), Value::Array(hosts));
    });

    experiment_config_parse_generic(test_cases, |test_case| {
        let hosts: Vec<String> = deserialize_value(&test_case.toml["hosts"]);

        let name: String = deserialize_constant(EXPERIMENT_CONFIG_DEFAULT_NAME);
        let script: PathBuf =
            deserialize_constant(EXPERIMENT_CONFIG_DEFAULT_SCRIPT);
        let description: String =
            deserialize_constant(EXPERIMENT_CONFIG_DEFAULT_DESCRIPTION);

        ExperimentConfig::new(name, description, script, &hosts)
    });
}

#[test]
fn experiment_config_parse_kind() {
    let start_table = build_table!(
        EXPERIMENT_CONFIG_DEFAULT_NAME,
        EXPERIMENT_CONFIG_DEFAULT_DESCRIPTION,
        EXPERIMENT_CONFIG_DEFAULT_SCRIPT,
        EXPERIMENT_CONFIG_DEFAULT_HOSTS
    );

    let tests = [
        (None, true, true),
        (Some(""), true, false),
        (Some("justmytype"), true, true),
    ];

    let test_cases = map_tests(&tests, start_table, |table, kind| {
        table.insert("kind".to_string(), Value::String(kind.to_string()));
    });

    experiment_config_parse_generic(test_cases, |test_case| {
        let kind: String = deserialize_value(&test_case.toml["kind"]);

        let name: String = deserialize_constant(EXPERIMENT_CONFIG_DEFAULT_NAME);
        let description: String =
            deserialize_constant(EXPERIMENT_CONFIG_DEFAULT_DESCRIPTION);
        let script: PathBuf =
            deserialize_constant(EXPERIMENT_CONFIG_DEFAULT_SCRIPT);
        let hosts: Vec<String> =
            deserialize_constant(EXPERIMENT_CONFIG_DEFAULT_HOSTS);

        let mut experiment_config =
            ExperimentConfig::new(name, description, script, &hosts);
        experiment_config.with_kind(kind);
        experiment_config
    });
}

#[test]
fn experiment_config_parse_dependencies() {
    let start_table = build_table!(
        EXPERIMENT_CONFIG_DEFAULT_NAME,
        EXPERIMENT_CONFIG_DEFAULT_DESCRIPTION,
        EXPERIMENT_CONFIG_DEFAULT_SCRIPT,
        EXPERIMENT_CONFIG_DEFAULT_HOSTS
    );

    let tests = [
        (None, true, true),
        (Some(vec![""]), true, false),
        (Some(vec!["dependency1.sh"]), true, true),
        (Some(vec!["dependency1.sh", "dependency2.sh"]), true, true),
        (
            Some(vec!["dependency1.sh", "dependency2.sh", ""]),
            true,
            false,
        ),
    ];

    let test_cases = map_tests(&tests, start_table, |table, dependencies| {
        let dependencies: Vec<Value> = dependencies
            .into_iter()
            .map(|dependency| Value::String(dependency.to_string()))
            .collect();
        table.insert("dependencies".to_string(), Value::Array(dependencies));
    });

    experiment_config_parse_generic(test_cases, |test_case| {
        let dependencies: Vec<PathBuf> =
            deserialize_value(&test_case.toml["dependencies"]);

        let name: String = deserialize_constant(EXPERIMENT_CONFIG_DEFAULT_NAME);
        let description: String =
            deserialize_constant(EXPERIMENT_CONFIG_DEFAULT_DESCRIPTION);
        let script: PathBuf =
            deserialize_constant(EXPERIMENT_CONFIG_DEFAULT_SCRIPT);
        let hosts: Vec<String> =
            deserialize_constant(EXPERIMENT_CONFIG_DEFAULT_HOSTS);

        let mut experiment_config =
            ExperimentConfig::new(name, description, script, &hosts);
        experiment_config.with_dependencies(&dependencies);
        experiment_config
    });
}

#[test]
fn experiment_config_parse_arguments() {
    let start_table = build_table!(
        EXPERIMENT_CONFIG_DEFAULT_NAME,
        EXPERIMENT_CONFIG_DEFAULT_DESCRIPTION,
        EXPERIMENT_CONFIG_DEFAULT_SCRIPT,
        EXPERIMENT_CONFIG_DEFAULT_HOSTS
    );

    let tests = [
        (None, true, true),
        (Some(vec![""]), true, true),
        (Some(vec!["arg1"]), true, true),
        (Some(vec!["arg1", "arg2"]), true, true),
        (Some(vec!["arg1", "", "arg3"]), true, true),
    ];

    let test_cases = map_tests(&tests, start_table, |table, arguments| {
        let arguments: Vec<Value> = arguments
            .into_iter()
            .map(|argument| Value::String(argument.to_string()))
            .collect();
        table.insert("arguments".to_string(), Value::Array(arguments));
    });

    experiment_config_parse_generic(test_cases, |test_case| {
        let arguments: Vec<String> =
            deserialize_value(&test_case.toml["arguments"]);

        let name: String = deserialize_constant(EXPERIMENT_CONFIG_DEFAULT_NAME);
        let description: String =
            deserialize_constant(EXPERIMENT_CONFIG_DEFAULT_DESCRIPTION);
        let script: PathBuf =
            deserialize_constant(EXPERIMENT_CONFIG_DEFAULT_SCRIPT);
        let hosts: Vec<String> =
            deserialize_constant(EXPERIMENT_CONFIG_DEFAULT_HOSTS);

        let mut experiment_config =
            ExperimentConfig::new(name, description, script, &hosts);
        experiment_config.with_arguments(&arguments);
        experiment_config
    });
}

#[test]
fn experiment_config_parse_expected_arguments() {
    let start_table = build_table!(
        EXPERIMENT_CONFIG_DEFAULT_NAME,
        EXPERIMENT_CONFIG_DEFAULT_DESCRIPTION,
        EXPERIMENT_CONFIG_DEFAULT_SCRIPT,
        EXPERIMENT_CONFIG_DEFAULT_HOSTS
    );

    let tests = [
        (None, true, true),
        (Some(0usize), true, true),
        (Some(1usize), true, false),
        (Some(100usize), true, false),
    ];

    let test_cases =
        map_tests(&tests, start_table, |table, expected_arguments| {
            table.insert(
                "expected_arguments".to_string(),
                Value::Integer(*expected_arguments as i64),
            );
        });

    experiment_config_parse_generic(test_cases, |test_case| {
        let expected_arguments: usize =
            deserialize_value(&test_case.toml["expected_arguments"]);

        let name: String = deserialize_constant(EXPERIMENT_CONFIG_DEFAULT_NAME);
        let description: String =
            deserialize_constant(EXPERIMENT_CONFIG_DEFAULT_DESCRIPTION);
        let script: PathBuf =
            deserialize_constant(EXPERIMENT_CONFIG_DEFAULT_SCRIPT);
        let hosts: Vec<String> =
            deserialize_constant(EXPERIMENT_CONFIG_DEFAULT_HOSTS);

        let mut experiment_config =
            ExperimentConfig::new(name, description, script, &hosts);
        experiment_config.with_expected_arguments(expected_arguments);
        experiment_config
    });
}

#[test]
fn experiment_config_parse_runs() {
    let start_table = build_table!(
        EXPERIMENT_CONFIG_DEFAULT_NAME,
        EXPERIMENT_CONFIG_DEFAULT_DESCRIPTION,
        EXPERIMENT_CONFIG_DEFAULT_SCRIPT,
        EXPERIMENT_CONFIG_DEFAULT_HOSTS
    );

    let tests = [
        (None, true, true),
        (Some(0u16), true, false),
        (Some(1u16), true, true),
        (Some(100u16), true, true),
        (Some(65535u16), true, true),
    ];

    let test_cases = map_tests(&tests, start_table, |table, runs| {
        table.insert("runs".to_string(), Value::Integer(*runs as i64));
    });

    experiment_config_parse_generic(test_cases, |test_case| {
        let runs: u16 = deserialize_value(&test_case.toml["runs"]);

        let name: String = deserialize_constant(EXPERIMENT_CONFIG_DEFAULT_NAME);
        let description: String =
            deserialize_constant(EXPERIMENT_CONFIG_DEFAULT_DESCRIPTION);
        let script: PathBuf =
            deserialize_constant(EXPERIMENT_CONFIG_DEFAULT_SCRIPT);
        let hosts: Vec<String> =
            deserialize_constant(EXPERIMENT_CONFIG_DEFAULT_HOSTS);

        let mut experiment_config =
            ExperimentConfig::new(name, description, script, &hosts);
        experiment_config.with_runs(runs as u16);
        experiment_config
    });
}

#[test]
fn experiment_config_parse_exporters() {
    let start_table = build_table!(
        EXPERIMENT_CONFIG_DEFAULT_NAME,
        EXPERIMENT_CONFIG_DEFAULT_DESCRIPTION,
        EXPERIMENT_CONFIG_DEFAULT_SCRIPT,
        EXPERIMENT_CONFIG_DEFAULT_HOSTS
    );

    let tests = [
        (None, true, true),
        (Some(vec![""]), true, false),
        (Some(vec!["test-exporter"]), true, true),
        (Some(vec!["test-exporter", "test-exporter-2"]), true, true),
        (Some(vec!["localhost", "test-exporter"]), true, false),
        (Some(vec!["test-exporter", ""]), true, false),
        (Some(vec!["test-runner", ""]), true, false),
    ];

    let test_cases = map_tests(&tests, start_table, |table, exporters| {
        let exporters: Vec<Value> = exporters
            .into_iter()
            .map(|exporter| Value::String(exporter.to_string()))
            .collect();
        table.insert("exporters".to_string(), Value::Array(exporters));
    });

    experiment_config_parse_generic(test_cases, |test_case| {
        let exporters: Vec<String> =
            deserialize_value(&test_case.toml["exporters"]);

        let name: String = deserialize_constant(EXPERIMENT_CONFIG_DEFAULT_NAME);
        let description: String =
            deserialize_constant(EXPERIMENT_CONFIG_DEFAULT_DESCRIPTION);
        let script: PathBuf =
            deserialize_constant(EXPERIMENT_CONFIG_DEFAULT_SCRIPT);
        let hosts: Vec<String> =
            deserialize_constant(EXPERIMENT_CONFIG_DEFAULT_HOSTS);

        let mut experiment_config =
            ExperimentConfig::new(name, description, script, &hosts);
        experiment_config.with_exporters(&exporters);
        experiment_config
    });
}

fn remote_execution_tests(
) -> Vec<(Option<[RemoteExecutionConfig; 1]>, bool, bool)> {
    vec![
        (None, true, true),
        (
            Some([RemoteExecutionConfig::new(
                "Basic command",
                &["test-runner"],
                CommandSource::from_command("echo hello"),
            )]),
            true,
            true,
        ),
        (
            Some([RemoteExecutionConfig::new(
                "Script",
                &["test-runner"],
                CommandSource::from_script("myscript.sh"),
            )]),
            true,
            true,
        ),
        (
            Some([RemoteExecutionConfig::new(
                "Script with dependency",
                &["test-runner"],
                CommandSource::from_script("myscript.sh"),
            )
            .with_dependencies(&["script_dependency.sh"])
            .clone()]),
            true,
            true,
        ),
        // (
        //     Some([RemoteExecutionConfig::new(
        //         ""
        //         &["test-runner"],
        //         CommandSource::from_script("myscript.sh"),
        //     )
        //     .with_dependencies(&["script_dependency.sh"])
        //     // We only use file_type internally, so it's always
        //     // set to the correct value (Setup)
        //     // .with_file_type(FileType::Setup)
        //     .clone()]),
        //     true,
        //     true,
        // ),
        (
            Some([RemoteExecutionConfig::new(
                "Command with explicitly no dependencies",
                &["test-runner"],
                CommandSource::from_command("echo hello"),
            )
            .with_dependencies::<PathBuf>(&[])
            .clone()]),
            true,
            true,
        ),
        (
            Some([RemoteExecutionConfig::new(
                "No hosts",
                &[],
                CommandSource::from_command("echo hello"),
            )]),
            true,
            false,
        ),
        (
            Some([RemoteExecutionConfig::new(
                "One empty host",
                &[""],
                CommandSource::from_command("echo hello"),
            )]),
            true,
            false,
        ),
        (
            Some([RemoteExecutionConfig::new(
                "Empty command",
                &["test-runner"],
                CommandSource::from_command(""),
            )]),
            true,
            false,
        ),
        (
            Some([RemoteExecutionConfig::new(
                "Command with one empty dependency",
                &["test-runner"],
                CommandSource::from_command("echo hello"),
            )
            .with_dependencies(&[""])
            .clone()]),
            true,
            false,
        ),
    ]
}

#[test]
fn experiment_config_parse_setup() {
    let start_table = build_table!(
        EXPERIMENT_CONFIG_DEFAULT_NAME,
        EXPERIMENT_CONFIG_DEFAULT_DESCRIPTION,
        EXPERIMENT_CONFIG_DEFAULT_SCRIPT,
        EXPERIMENT_CONFIG_DEFAULT_HOSTS
    );

    let mut tests = remote_execution_tests();

    for test in tests.iter_mut() {
        if let Some(ref mut res) = test.0 {
            for re in res.iter_mut() {
                re.with_file_type(FileType::Setup);
            }
        }
    }

    let test_cases = map_tests(&tests, start_table, |table, setups| {
        let setups: Vec<Value> = setups
            .into_iter()
            .map(|setup| {
                Value::try_from(setup)
                    .expect("Failed to serialize setup to Value")
            })
            .collect();
        table.insert("setups".to_string(), Value::Array(setups));
    });

    experiment_config_parse_generic(test_cases, |test_case| {
        let mut setups: Vec<RemoteExecutionConfig> =
            deserialize_value(&test_case.toml["setups"]);

        let setups: Vec<RemoteExecutionConfig> = setups
            .iter_mut()
            .map(|setup: &mut RemoteExecutionConfig| {
                setup.with_file_type(FileType::Setup).clone()
            })
            .collect();

        let name: String = deserialize_constant(EXPERIMENT_CONFIG_DEFAULT_NAME);
        let description: String =
            deserialize_constant(EXPERIMENT_CONFIG_DEFAULT_DESCRIPTION);
        let script: PathBuf =
            deserialize_constant(EXPERIMENT_CONFIG_DEFAULT_SCRIPT);
        let hosts: Vec<String> =
            deserialize_constant(EXPERIMENT_CONFIG_DEFAULT_HOSTS);

        let mut experiment_config =
            ExperimentConfig::new(name, description, script, &hosts);
        experiment_config.with_setup(&setups);
        experiment_config
    });
}

#[test]
fn experiment_config_parse_teardown() {
    let start_table = build_table!(
        EXPERIMENT_CONFIG_DEFAULT_NAME,
        EXPERIMENT_CONFIG_DEFAULT_DESCRIPTION,
        EXPERIMENT_CONFIG_DEFAULT_SCRIPT,
        EXPERIMENT_CONFIG_DEFAULT_HOSTS
    );

    let mut tests = remote_execution_tests();

    for test in tests.iter_mut() {
        if let Some(ref mut res) = test.0 {
            for re in res.iter_mut() {
                re.with_file_type(FileType::Teardown);
            }
        }
    }

    let test_cases = map_tests(&tests, start_table, |table, teardowns| {
        let teardowns: Vec<Value> = teardowns
            .into_iter()
            .map(|teardown| {
                Value::try_from(teardown)
                    .expect("Failed to serialize teardown to Value")
            })
            .collect();
        table.insert("teardowns".to_string(), Value::Array(teardowns));
    });

    experiment_config_parse_generic(test_cases, |test_case| {
        let mut teardowns: Vec<RemoteExecutionConfig> =
            deserialize_value(&test_case.toml["teardowns"]);

        let teardowns: Vec<RemoteExecutionConfig> = teardowns
            .iter_mut()
            .map(|teardown: &mut RemoteExecutionConfig| {
                teardown.with_file_type(FileType::Teardown).clone()
            })
            .collect();

        let name: String = deserialize_constant(EXPERIMENT_CONFIG_DEFAULT_NAME);
        let description: String =
            deserialize_constant(EXPERIMENT_CONFIG_DEFAULT_DESCRIPTION);
        let script: PathBuf =
            deserialize_constant(EXPERIMENT_CONFIG_DEFAULT_SCRIPT);
        let hosts: Vec<String> =
            deserialize_constant(EXPERIMENT_CONFIG_DEFAULT_HOSTS);

        let mut experiment_config =
            ExperimentConfig::new(name, description, script, &hosts);
        experiment_config.with_teardown(&teardowns);
        experiment_config
    });
}

#[test]
fn experiment_config_parse_variations() {
    let start_table = build_table!(
        EXPERIMENT_CONFIG_DEFAULT_NAME,
        EXPERIMENT_CONFIG_DEFAULT_DESCRIPTION,
        EXPERIMENT_CONFIG_DEFAULT_SCRIPT,
        EXPERIMENT_CONFIG_DEFAULT_HOSTS
    );

    let tests = [
        (None, true, true),
        (
            Some(vec![VariationConfig::new("test-variation")
                .with_description("A simple variation for testing")
                .with_hosts(&["test-runner"])
                .with_expected_arguments(3)
                .with_arguments(&["hello", "to", "you"])
                .with_exporters(&vec!["test-exporter"])
                .clone()]),
            true,
            true,
        ),
        (
            Some(vec![
                VariationConfig::new("test-variation")
                    .with_hosts(&["test-runner"])
                    .with_expected_arguments(3)
                    .with_arguments(&["hello", "to", "you"])
                    .with_exporters(&vec!["test-exporter"])
                    .clone(),
                VariationConfig::new("test-variation2")
                    .with_hosts(&["localhost"])
                    .clone(),
            ]),
            true,
            true,
        ),
        (
            Some(vec![VariationConfig::new("test-variation")
                .with_description("")
                .with_hosts(&["test-runner"])
                .clone()]),
            true,
            false,
        ),
        (
            Some(vec![VariationConfig::new("test-variation")
                .with_description("A simple variation for testing")
                .with_hosts(&[""])
                .clone()]),
            true,
            false,
        ),
        (
            Some(vec![VariationConfig::new("test-variation")
                .with_description("A simple variation for testing")
                .with_expected_arguments(4)
                .with_arguments(&["hello", "to", "you"])
                .clone()]),
            true,
            false,
        ),
        (
            Some(vec![VariationConfig::new("test-variation")
                .with_description("A simple variation for testing")
                .with_arguments::<&str>(&[])
                .clone()]),
            true,
            true,
        ),
        (
            Some(vec![VariationConfig::new("test-variation")
                .with_description("A simple variation for testing")
                .with_exporters(&vec![""])
                .clone()]),
            true,
            false,
        ),
        (
            Some(vec![VariationConfig::new("test-variation")
                .with_description("A simple variation for testing")
                .with_hosts(&["test-runner", "nonexistant-runner"])
                .clone()]),
            true,
            false,
        ),
        (
            Some(vec![VariationConfig::new("test-variation")]),
            true,
            false,
        ),
        (
            // expected_arguments by itself should not constitute a valid variation!
            Some(vec![VariationConfig::new("test-variation")
                .with_expected_arguments(0)
                .clone()]),
            true,
            false,
        ),
    ];

    let test_cases = map_tests(&tests, start_table, |table, variations| {
        let variations: Vec<Value> = variations
            .into_iter()
            .map(|variation| {
                Value::try_from(variation)
                    .expect("Failed to serialize variation to Value")
            })
            .collect();
        table.insert("variations".to_string(), Value::Array(variations));
    });

    experiment_config_parse_generic(test_cases, |test_case| {
        let variations: Vec<VariationConfig> =
            deserialize_value(&test_case.toml["variations"]);

        let name: String = deserialize_constant(EXPERIMENT_CONFIG_DEFAULT_NAME);
        let description: String =
            deserialize_constant(EXPERIMENT_CONFIG_DEFAULT_DESCRIPTION);
        let script: PathBuf =
            deserialize_constant(EXPERIMENT_CONFIG_DEFAULT_SCRIPT);
        let hosts: Vec<String> =
            deserialize_constant(EXPERIMENT_CONFIG_DEFAULT_HOSTS);

        let mut experiment_config =
            ExperimentConfig::new(name, description, script, &hosts);
        experiment_config.with_variations(&variations);
        experiment_config
    });
}

#[test]
// these tests check for wider integration between multiple different valid config files
fn valid_config_files() {
    for entry in configs_path()
        .read_dir()
        .expect("Failed to read configs directory")
    {
        let entry = entry.expect("Failed to read directory entry");
        let file_name = entry.file_name();

        if file_name.to_string_lossy().starts_with("config") {
            read_in_config(&entry.path());
        }
    }
}

#[test]
// these tests check for wider integration between multiple different valid config files
fn valid_experiment_config_files() {
    let config = read_in_default_config();

    for entry in configs_path()
        .read_dir()
        .expect("Failed to read configs directory")
    {
        let entry = entry.expect("Failed to read directory entry");
        let file_name = entry.file_name();

        if file_name.to_string_lossy().starts_with("experiment_config") {
            read_in_experiment_config(&entry.path(), &config);
        }
    }
}

#[test]
fn to_experiments() {
    let (config, experiment_config) = read_in_default_configs();

    let experiments =
        generate_experiments(&config, &experiment_config, test_path()).unwrap();

    let basepath = test_path();

    let re_host = Host {
        name: "test-runner".to_string(),
        address: "testuser@testserver.internal".to_string(),
    };

    let scripts: HashMap<&str, (String, PathBuf)> = HashMap::from([
        (
            "setup",
            (
                String::from("main-setup"),
                basepath.join("setup/test-setup.sh"),
            ),
        ),
        (
            "teardown",
            (
                String::from("main-teardown"),
                basepath.join("teardown/test-teardown.sh"),
            ),
        ),
        (
            "execute",
            (
                String::from("execute"),
                basepath.join("execute/actual-work.sh"),
            ),
        ),
    ]);

    let mut remote_executions: HashMap<&str, Vec<RemoteExecution>> =
        HashMap::with_capacity(scripts.len());
    for (stage, (name, script)) in scripts.iter() {
        remote_executions.insert(
            stage,
            vec![RemoteExecution {
                name: name.clone(),
                hosts: vec![re_host.clone()],
                command: CommandSource::Script(script.clone()),
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
                command: CommandSource::from_script("sarexporter.sh"),
                setup: Some(CommandSource::from_command(
                    "dnf install -y sysstat; apt install -y sysstat",
                )),
            },
        ),
        (
            "test-exporter-2",
            Exporter {
                name: "test-exporter-2".into(),
                hosts: vec![Host {
                    name: "localhost".into(),
                    address: "localhost".into(),
                }],
                command: CommandSource::from_command("my exporter command"),
                setup: Some(CommandSource::from_script(
                    "exporter_prep_script.sh",
                )),
            },
        ),
    ]);

    let dependencies: Vec<PathBuf> =
        vec![basepath.join("dependency/dependency.sh")];

    let expected_experiments = vec![
            Experiment {
                id: None,
                name: "localhost-experiment".to_string(),
                description: "Testing the functionality of the software completely using localhost".into(),
                kind: "localhost-result".into(),
                setups: remote_executions["setup"].clone(),
                teardowns: remote_executions["teardown"].clone(),
                hosts: vec![re_host.clone()],
                execute: remote_executions["execute"].first().unwrap().clone(),
                dependencies: dependencies.clone(),
                expected_arguments: Some(2),
                arguments: vec!["Argument 1".into(), "Argument 2".into()],
                exporters: vec![]
            },
            Experiment {
                id: None,
                name: "localhost-experiment-different args".to_string(),
                description: "A completely new description".into(),
                kind: "localhost-result".into(),
                setups: remote_executions["setup"].clone(),
                teardowns: remote_executions["teardown"].clone(),
                hosts: vec![re_host.clone()],
                execute: remote_executions["execute"].first().unwrap().clone(),
                dependencies: dependencies.clone(),
                arguments: vec!["Argument 1".into()],
                expected_arguments: Some(1),
                exporters: vec![]
            },
            Experiment {
                id: None,
                name: "localhost-experiment-with exporter".to_string(),
                description: "Testing the functionality of the software completely using localhost".into(),
                kind: "localhost-result".into(),
                setups: remote_executions["setup"].clone(),
                teardowns: remote_executions["teardown"].clone(),
                hosts: vec![re_host.clone()],
                execute: remote_executions["execute"].first().unwrap().clone(),
                dependencies: dependencies.clone(),
                expected_arguments: Some(2),
                arguments: vec!["Argument 1".into(), "Argument 2".into()],
                exporters: vec![exporters["test-exporter"].clone()]
            },
            Experiment {
                id: None,
                name: "localhost-experiment-with multiple exporters".to_string(),
                description: "Testing the functionality of the software completely using localhost".into(),
                kind: "localhost-result".into(),
                setups: remote_executions["setup"].clone(),
                teardowns: remote_executions["teardown"].clone(),
                hosts: vec![re_host.clone()],
                execute:  remote_executions["execute"].first().unwrap().clone(),
                dependencies: dependencies.clone(),
                expected_arguments: Some(2),
                arguments: vec!["Argument 1".into(), "Argument 2".into()],
                exporters: vec![exporters["test-exporter"].clone(), exporters["test-exporter-2"].clone()]
            },
            Experiment {
                id: None,
                name: "localhost-experiment-with different host".to_string(),
                description: "Testing the functionality of the software completely using localhost".into(),
                kind: "localhost-result".into(),
                setups: remote_executions["setup"].clone(),
                teardowns: remote_executions["teardown"].clone(),
                hosts: vec![Host::localhost()],
                execute:  remote_executions["execute"].first().unwrap().clone(),
                dependencies: dependencies.clone(),
                expected_arguments: Some(2),
                arguments: vec!["Argument 1".into(), "Argument 2".into()],
                exporters: vec![]
            },
        ];

    let expected_runs = 1;
    assert_eq!(experiments.0, expected_runs);

    assert!(
        experiments.1.len() == expected_experiments.len(),
        "There are variations that aren't being tested!"
    );

    for (experiment, expected_experiment) in
        experiments.1.iter().zip(expected_experiments)
    {
        assert!(
            experiment == &expected_experiment,
            "{}",
            to_string_side_by_side(
                "Experiment Config",
                experiment,
                &expected_experiment,
                63
            )
        );
    }
}
