<script lang="ts">
	import { Button } from '$lib/components/ui/button/index.js';
	import * as Card from '$lib/components/ui/card/index.js';
	import { Play, Trash, Edit } from '@lucide/svelte';
	import * as Select from '$lib/components/ui/select/index.js';

	const infrastructures = [
		{ value: 'apple', label: 'Beans.toml' },
		{ value: 'banana', label: 'Beans2.toml' },
		{ value: 'blueberry', label: 'Beans3.toml' },
		{ value: 'grapes', label: 'Beans4.toml' },
		{ value: 'pineapple', label: 'Beans5.toml' }
	];

	let value = $state('');

	const triggerContent = $derived(
		infrastructures.find((f) => f.value === value)?.label ?? 'Select infrastructure configuration'
	);
</script>

<Card.Root>
	<Card.Header>
		<Card.Title class="relative">
			Beans.toml
			<div class="absolute flex gap-4 right-0 -top-1">
				<Trash class="w-4 cursor-pointer"></Trash>
				<Edit class="w-4 cursor-pointer"></Edit>
			</div>
		</Card.Title>
		<Card.Description>Modified 01/07/2025 01:00 PM</Card.Description>
	</Card.Header>
	<Card.Content class="flex flex-col gap-4">
		<Select.Root type="single" name="favoriteFruit" bind:value>
			<Select.Trigger class="w-full">
				{triggerContent}
			</Select.Trigger>
			<Select.Content>
				<Select.Group>
					{#each infrastructures as infrastructure (infrastructure.value)}
						<Select.Item value={infrastructure.value} label={infrastructure.label}>
							{infrastructure.label}
						</Select.Item>
					{/each}
				</Select.Group>
			</Select.Content>
		</Select.Root>
		<Button>
			Run Experiment
			<Play></Play>
		</Button>
	</Card.Content>
</Card.Root>
