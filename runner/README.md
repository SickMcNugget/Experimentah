# Runner 

The runner is the software component responsible for executing the experiment, and saving the results. 

## Workflow

1. Start the runner - this creates a rust backend handler at localhost:50000, and an endpoint at localhost:50001.
2. Controller posts at /add_experiment to create a new experiment in the runner. 
- Request contains a experiment_id (name of experiment + metadata (ts, repeat_num etc.)) and a JSON body that holds the list of commands to run and the required exporters. 
3. Runner sets up the required exporters (on designated **hosts** (nodes) from request body).
- Exporters collect data from experiment outputs (write to file on local system). 
4. Runner runs the designated commands from a provided bash script. The bash script is stored in the workload repository, but is passed in the request body along with any arguments.  
5. After the experiments have finished, the output data for each host is collected as soon as that host finishes (bash script terminates). 
6. When all the data has been collected, the runner informs the brain via a post request. The brain then collects the data from this run, and adds the local data locations (on the brain) to a central database. 
