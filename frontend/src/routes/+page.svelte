<script lang="ts">
	import Status from './Status.svelte';
	import Tabs from './Tabs.svelte';
</script>

<section class="max-w-[65rem] pt-12 mx-auto">
	<h1 class="text-4xl font-bold">Experiment Dashboard</h1>
	<h2 class="text-xl text-secondary-foreground">Manage and run experiments</h2>
</section>

<section class="max-w-[65rem] pt-8 mx-auto">
	<Status></Status>
</section>

<section class="max-w-[65rem] pt-8 mx-auto">
	<Tabs></Tabs>
</section>
