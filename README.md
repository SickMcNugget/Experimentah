# Controller
The controller is the user portion of the suite.

## Config
The config file (yes, singular) contains all the static information potentially needed
by experiments down the track. This includes reachable hosts, available exporters, etc.

```toml
# An example config file
hosts = ["my-host.com:2000", "my-host2.com:2001"]

[[exporters]]
name = "test-exporter"

[[exporters]]
name = "other-exporter"
```

## Experiments
Experiment files are used to actually run experiments, what a crazy concept!
They contain a list of jobs, which correspond to the smallest unit of execution.
For now, jobs can only be run sequentially, limiting potential applications. 

```toml
# An example experiment file with two jobs
name = "test-experiment"
result_type = "test-result"
runs = 1 # default

[[jobs]]
name = "test-job"
exporters = ["test-exporter", "other-exporter"]
setup = ["test-setup.sh"]
execute = ["actual-work.sh"]
teardown = ["test-teardown.sh"]

[[jobs]]
name = "test-job2"
exporters = ["test-exporter"]
setup = ["test-setup.sh"]
execute = ["actual-work.sh"]
teardown = ["test-teardown.sh"]

[[jobs]]
name = "test-job3"
exporters = ["test-exporter"]
execute = ["actual-work.sh"]
```

# Experiment API
Contains the functionality of the master node.
This includes metric reporting and the workload repository.
Coordinates the initialisation of experiments. This will become
more significant when concurrent execution is allowed in the future.

## Postgres
Postgres is used for the raw outputs produced by jobs.
Note that the console outputs are stored in a file that is pointed to by the DB.

## Prometheus
Prometheus is used for time-series data.
This time series data from the exporters defined in the Controller config.

## Metrics Reporter
Part of the experiment API. Takes in raw metrics and inserts them where they need to end up.

## Workload repository
A simple file server.
The workload repository contains job scripts and their dependencies.
The workload repository is part of the experiment API.

# Runner
The runner should be placed on the nodes that actually need to run jobs.
It is responsible for:
- pulling in job scripts/requirements from the workload repository,
- Running the commands defined by the setup, execute and teardown scripts,
- Reporting the result of the jobs to the experiment API.
