<script lang="ts">
	import * as Tabs from '$lib/components/ui/tabs/index.js';
	import Upload from './Upload.svelte';
	import InfrastructureCard from './InfrastructureCard.svelte';
	import ExperimentCard from './ExperimentCard.svelte';
	import History from './History.svelte';
	import {
		infrastructureConfigs,
		experimentConfigs,
		handleUpload,
		handleDelete
	} from './configs.svelte';
</script>

<Tabs.Root value="configuration" class="mb-8">
	<Tabs.List class="p-1 mx-auto border-1 rounded-lg border-input">
		<Tabs.Trigger value="configuration" class="text-md px-10">Configuration</Tabs.Trigger>
		<Tabs.Trigger value="history" class="text-md px-10">History</Tabs.Trigger>
	</Tabs.List>

	<Tabs.Content value="configuration" class="mt-2">
		<!-- nested tabs -->
		<Tabs.Root value="infrastructure">
			<Tabs.List>
				<Tabs.Trigger value="infrastructure">Infrastructure</Tabs.Trigger>
				<Tabs.Trigger value="experiments">Experiments</Tabs.Trigger>
			</Tabs.List>
			<Tabs.Content value="infrastructure" class="mt-2">
				<Upload
					configType="infrastructure"
					onUpload={(file) => handleUpload(file, 'infrastructure')}
				></Upload>

				<div class="grid grid-cols-3 gap-4 mt-4">
					{#each infrastructureConfigs as config (config.name)}
						<InfrastructureCard
							name={config.name}
							modifiedDate={config.modifiedDate}
							content={config.content}
							onDelete={() => handleDelete(config.name, 'infrastructure')}
						></InfrastructureCard>
					{/each}
				</div>
			</Tabs.Content>
			<Tabs.Content value="experiments" class="mt-2">
				<Upload configType="experiment" onUpload={(file) => handleUpload(file, 'experiment')}
				></Upload>

				<div class="grid grid-cols-3 gap-4 mt-4">
					{#each experimentConfigs as config (config.name)}
						<ExperimentCard
							name={config.name}
							modifiedDate={config.modifiedDate}
							content={config.content}
							onDelete={() => handleDelete(config.name, 'experiment')}
						></ExperimentCard>
					{/each}
				</div>
			</Tabs.Content>
		</Tabs.Root>
		<!-- nested tabs ends here -->
	</Tabs.Content>
	<Tabs.Content value="history" class="mt-2">
		<History></History>
	</Tabs.Content>
</Tabs.Root>
