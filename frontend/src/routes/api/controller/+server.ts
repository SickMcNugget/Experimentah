import { json } from '@sveltejs/kit';

// TODO(jackson): this should actually conntect to the controller api to get the state
export async function GET() {
	// randomly choose between unknown, running
	const status = ['unknown', 'running', 'error'][Math.floor(Math.random() * 3)];
	// status = 'running';

	return json({
		status,
		infrastructureConfig:
			status === 'running' ? { name: 'infrastructure-config.toml', content: 'test' } : null,
		experimentConfig:
			status === 'running' ? { name: 'experiment-config.toml', content: 'test' } : null,
		error: status === 'error' ? 'An error occurred' : null
	});
}
