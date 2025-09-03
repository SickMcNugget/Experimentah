# Controller
The controller is the user portion of the suite.

## Config
The main config (referred to as Config) file serves as a registry that can be reused and
grown as time goes on and requires it. This file should simply contain the hosts and exporters
that can be reused across many experiments.

Below is an example config.
```toml
[[hosts]]
name = "test-runner"
address = "testuser@testserver.internal"

[[exporters]]
name = "test-exporter"
hosts = ["test-runner"]
script = "sarexporter.sh"

[[exporters]]
name = "test-exporter-2"
hosts = ["localhost"]
command = "my exporter command"
setup.script = "exporter_prep_script.sh"
```
There are a few important features of the config file to mention:
- The "localhost" address is reserved, and is always available.
  As such, it cannot be defined in the \[\[hosts\]\] section
- "script" and command are mutually exclusive and can always be
  substituted for one another. If you use both, this is undefined
  behaviour, and one will be chosen over the other.

Some fields are required, and others are not. They are listed below

| field     | required? |
|--------------- | --------------- |
| hosts | false |
| hosts.name | true |
| hosts.address | true |
| exporters| false |
| exporters.name | true |
| exporters.hosts | true |
| exporters.command | one of these 2 |
| exporters.script | one of these 2 |
| exporters.setup | false |
| exporters.setup.command | one of these 2 |
| exporters.setup.script | one of these 2 |

Note that this does indicate that you can have an empty configuration file.
The only time this would be useful is if you are running an experiment on localhost,
and that experiment does not require any exporters.

## Experiments
Experiment Config files are used to define experiments.
The make reference to the aforementioned main config, and refer to exporters/hosts
by their shorthand names, instead of having to re-list all their fields.

Below is an example experiment config.
```toml
name = "localhost-experiment"
description = "Testing the functionality of the software completely using localhost"
script = "actual-work.sh"
hosts = ["test-runner"]

kind = "localhost-result"

dependencies = ["dependency.sh"]
arguments = ["Argument 1", "Argument 2"]
expected_arguments = 2

runs = 1

exporters = []

[[setups]]
hosts = ["test-runner"]
script = "test-setup.sh"
dependencies = ["setup-dependency.sh"]

[[teardowns]]
hosts = ["test-runner"]
script = "test-teardown.sh"
dependencies = ["teardown-dependency.sh"]

[[variations]]
name = "different args"
description = "A completely new description"
expected_arguments = 1
arguments = ["Argument 1"]

[[variations]]
name = "with exporter"
exporters = ["test-exporter"]

[[variations]]
name = "with multiple exporters"
exporters = ["test-exporter", "test-exporter-2"]

[[variations]]
name = "with different host"
hosts = ["localhost"]
```
Some important notes regarding experiment configurations are:
- variations represent an additional run of an experiment with
  some aspect of the experiment modified, whether this be the
  hosts the experiment runs on, the arguments for the experiment,
  or the exporters collecting during the experiment.
- setups run, in order of definition, before each variation of 
  the experiment. These can be used to prepare hosts in some way
  before running a main experiment script. Teardowns are the same,
  but run just after each variation of the experiment finishes.

Some fields are required, and others are not. They are listed below.

| field     | required? |
|--------------- | --------------- |
| name   | true |
| description   | true |
| script   | true |
| hosts   | true   |
| kind | false   |
| dependencies | false   |
| arguments | false   |
| expected_arguments | false   |
| runs | false   |
| exporters | false   |
| setups | false   |
| setups.hosts | true   |
| setups.script | one of these 2 |
| setups.command | one of these 2 |
| setups.dependencies | false |
| variations| false   |
| variations.name | true   |
| variations.description | false   |
| variations.expected_arguments | one of these 4 |
| variations.arguments | one of these 4 |
| variations.exporters | one of these 4 |
| variations.hosts     | one of these 4 |

