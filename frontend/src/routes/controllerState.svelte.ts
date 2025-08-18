// Hold a reactive object representing the state of the controller (external service)
// This just makes it easier for the UI to have access to what is going on

export type InfrastructureConfig = {
	name: string;
	content: string;
};

export type ExperimentConfig = {
	name: string;
	content: string;
};

export type ControllerState = {
	status: 'idle' | 'running' | 'error' | 'unknown';
	infrastructureConfig: InfrastructureConfig | null;
	experimentConfig: ExperimentConfig | null;
	error: string | null;
};

export const controllerState = $state<ControllerState>({
	status: 'unknown',
	infrastructureConfig: null,
	experimentConfig: null,
	error: null
});

let interval: NodeJS.Timeout | null = null;

// TODO(jackson): env var for polling interval
// Call this only once to start polling for the controller state
export async function startPolling() {
	if (interval) {
		stopPolling();
	}

	interval = setInterval(async () => {
		const response = await fetch('/api/controller');
		const data = await response.json();
		controllerState.status = data.status;
		controllerState.infrastructureConfig = data.infrastructureConfig;
		controllerState.experimentConfig = data.experimentConfig;
		controllerState.error = data.error;
	}, 2500);
}

export async function stopPolling() {
	if (interval) {
		clearInterval(interval);
		interval = null;
	}
}
