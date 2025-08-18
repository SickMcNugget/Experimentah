<script lang="ts">
	import { Button } from '$lib/components/ui/button/index.js';
	import * as Card from '$lib/components/ui/card/index.js';
	import { Square } from '@lucide/svelte';
	import { controllerState, startPolling, stopPolling } from './controllerState.svelte';

	$effect(() => {
		startPolling();

		// Stop polling when the component is unmounted
		return () => {
			stopPolling();
		};
	});
</script>

<!-- if the controller status is unknown show red dot with text Controller Status Unknown -->
<div>
	<Card.Root class="w-full">
		<Card.Header>
			<Card.Title>
				{#if controllerState.status === 'idle'}
					<div class="flex gap-2 items-center">
						<div class="w-4 h-4 bg-yellow-600 rounded-full"></div>
						Idle
					</div>
				{:else if controllerState.status === 'running'}
					<div class="flex gap-2 items-center">
						<div class="w-4 h-4 bg-green-600 rounded-full"></div>
						Running<span class="text-sm text-gray-500">(5m 20s)</span>
					</div>
					<div class="flex flex-col gap-2 pt-2">
						<div class="flex gap-2 items-center">
							<div class="font-medium text-sm">Infrastructure:</div>
							<div class="text-sm text-gray-500">{controllerState.infrastructureConfig?.name}</div>
						</div>
						<div class="flex gap-2 items-center">
							<div class="font-medium text-sm">Experiment:</div>
							<div class="text-sm text-gray-500">{controllerState.experimentConfig?.name}</div>
						</div>
					</div>
				{:else if controllerState.status === 'error'}
					<div class="flex gap-2 items-center">
						<div class="w-4 h-4 bg-red-600 rounded-full"></div>
						Error
						<div class="text-sm text-red-600">{controllerState.error}</div>
					</div>
				{:else}
					<div class="flex gap-2 items-center">
						<div class="w-4 h-4 bg-red-600 rounded-full"></div>
						Unknown
					</div>
				{/if}
			</Card.Title>
			<Card.Description>{controllerState.status} for 5m 20s</Card.Description>
			<Card.Action>
				<Button variant="destructive">
					Stop
					<Square></Square>
				</Button>
			</Card.Action>
		</Card.Header>
	</Card.Root>
</div>
