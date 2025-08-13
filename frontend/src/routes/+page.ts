import type { PageLoad } from './$types';
import type { ConfigInfo } from '$lib/server/configs';

export const load: PageLoad = async ({ fetch }) => {
	const [infrastructureResponse, experimentResponse] = await Promise.all([
		fetch('/api/infrastructure-configs'),
		fetch('/api/experiment-configs')
	]);

	if (!infrastructureResponse.ok || !experimentResponse.ok) {
		return { infrastructureConfigs: [], experimentConfigs: [] };
	}

	const infrastructureConfigs: ConfigInfo[] = await infrastructureResponse.json();
	const experimentConfigs: ConfigInfo[] = await experimentResponse.json();

	return { infrastructureConfigs, experimentConfigs };
};



