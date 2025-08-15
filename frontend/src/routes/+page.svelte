<script lang="ts">
	import Status from './Status.svelte';
	import Tabs from './Tabs.svelte';
	import type { PageData } from './$types';
	import { infrastructureConfigs, experimentConfigs } from './configs.svelte';

	let { data }: { data: PageData } = $props();

	infrastructureConfigs.length = 0;
	infrastructureConfigs.push(...data.infrastructureConfigs);

	experimentConfigs.length = 0;
	experimentConfigs.push(...data.experimentConfigs);
</script>

<!-- heading -->
<section class="max-w-[65rem] pt-12 mx-auto">
	<h1 class="text-4xl font-bold">Experiment Dashboard</h1>
	<h2 class="text-xl text-secondary-foreground">Manage and run experiments</h2>
</section>

<!-- status -->
<section class="max-w-[65rem] pt-8 mx-auto">
	<Status></Status>
</section>

<!-- tabs -->
<section class="max-w-[65rem] pt-8 mx-auto">
	<Tabs></Tabs>
</section>
