import type { ConfigInfo } from '$lib/server/configs';

export const infrastructureConfigs = $state<ConfigInfo[]>([]);
export const experimentConfigs = $state<ConfigInfo[]>([]);

export async function handleUpload(file: File, type: 'infrastructure' | 'experiment') {
	const form = new FormData();
	form.append('file', file);

	const response = await fetch(`/api/${type}-configs`, { method: 'POST', body: form });

	if (!response.ok) {
		// TODO(jackson): show error on ui
		console.error('Failed to upload config', response);
		return;
	}

	const createdConfig = await response.json();

	if (type === 'infrastructure') {
		infrastructureConfigs.push(createdConfig);
	} else {
		experimentConfigs.push(createdConfig);
	}
}

export async function handleDelete(name: string, type: 'infrastructure' | 'experiment') {
	const response = await fetch(`/api/${type}-configs?name=${encodeURIComponent(name)}`, {
		method: 'DELETE'
	});

	if (!response.ok) {
		console.error('Failed to delete config', response);
		return;
	}

	if (type === 'infrastructure') {
		const filteredInfrastructureConfigs = infrastructureConfigs.filter((c) => c.name !== name);
		infrastructureConfigs.length = 0;
		infrastructureConfigs.push(...filteredInfrastructureConfigs);
	} else {
		const filteredExperimentConfigs = experimentConfigs.filter((c) => c.name !== name);
		experimentConfigs.length = 0;
		experimentConfigs.push(...filteredExperimentConfigs);
	}
}

export async function handleModify(
	name: string,
	content: string,
	type: 'infrastructure' | 'experiment'
) {
	// Just use the upload function but create a file from the content
	// TODO(jackson): could use patch for this but we would just be replacing the file on filesystem anyway
	const file = new File([content], name, { type: 'text/plain' });
	await handleUpload(file, type);
}
