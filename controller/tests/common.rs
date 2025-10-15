use controller::parse::{Config, ExperimentConfig};
use std::path::{Path, PathBuf};

pub const VALID_CONFIG: &str = "config_valid.toml";
pub const VALID_EXPERIMENT_CONFIG: &str = "experiment_config_valid.toml";

pub fn test_path() -> PathBuf {
    PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap()).join("tests")
}

pub fn configs_path() -> PathBuf {
    test_path().join("configs")
}

pub fn to_string_side_by_side<A: std::fmt::Display + std::fmt::Debug>(
    struct_name: &str,
    a: &A,
    b: &A,
    padding: usize,
) -> String {
    let mut result = String::new();
    let a_lines: Vec<String> =
        a.to_string().lines().map(String::from).collect();
    let b_lines: Vec<String> =
        b.to_string().lines().map(String::from).collect();

    let max_lines = a_lines.len().max(b_lines.len());
    let table_row_str = "-".repeat(padding);

    result.push_str(&format!(
        "Actual {struct_name:<width_a$} | Expected {struct_name:<width_b$}\n",
        width_a = padding - 7,
        width_b = padding - 9
    ));
    result.push_str(&format!(
        "{table_row_str:<width$} | {table_row_str:<width$}\n",
        width = padding
    ));

    for i in 0..max_lines {
        let default = String::new();
        let a_line = a_lines.get(i).unwrap_or(&default);
        let b_line = b_lines.get(i).unwrap_or(&default);
        if a_line != b_line {
            result.push_str(&format!(
                "\x1b[0;26m{a_line:<width$}\x1b[0m | \x1b[0;31m{b_line:<width$}\x1b[0m\n", width=padding
            ));
        } else {
            result.push_str(&format!(
                "{a_line:<width$} | {b_line:<width$}\n",
                width = padding
            ));
        }
    }

    result
}

pub fn read_in_config(path: &Path) -> Config {
    let config = match Config::from_file(path) {
        Ok(config) => config,
        Err(e) => panic!("A valid config couldn't be parsed: {e}"),
    };

    if let Err(e) = config.validate() {
        panic!("Unable to validate config: {e}");
    }

    config
}

pub fn read_in_experiment_config(
    path: &Path,
    config: &Config,
) -> ExperimentConfig {
    let experiment_config = match ExperimentConfig::from_file(path) {
        Ok(experiment_config) => experiment_config,
        Err(e) => panic!("A valid config couldn't be parsed: {e}"),
    };

    if let Err(e) = experiment_config.validate(config) {
        panic!("Unable to validate config: {e}");
    }

    experiment_config
}

pub fn read_in_default_config() -> Config {
    read_in_config(&configs_path().join(VALID_CONFIG))
}

pub fn read_in_default_configs() -> (Config, ExperimentConfig) {
    let config = read_in_default_config();
    let experiment_config = read_in_experiment_config(
        &configs_path().join(VALID_EXPERIMENT_CONFIG),
        &config,
    );

    (config, experiment_config)
}
