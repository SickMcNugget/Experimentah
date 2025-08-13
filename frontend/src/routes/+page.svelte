<script lang="ts">
	import Status from './Status.svelte';
	import Tabs from './Tabs.svelte';
	import { Button } from '$lib/components/ui/button';
	import type { PageData } from './$types';

	let { data }: { data: PageData } = $props();
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
	<Tabs data={data}></Tabs>
</section>
