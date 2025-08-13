<script lang="ts">
	import { Button } from '$lib/components/ui/button/index.js';
	import * as Card from '$lib/components/ui/card/index.js';
	import { Play, Trash, Edit } from '@lucide/svelte';
	import * as Select from '$lib/components/ui/select/index.js';
    

	let { name, modifiedDate, content, onDelete }: { name: string; modifiedDate: string; content?: string; onDelete?: () => void } = $props();
	// TODO(jackson): add editor dialog
</script>

<Card.Root>
	<Card.Header>
		<Card.Title class="relative">
			{name}
			<div class="absolute flex gap-4 right-0 -top-1">
				<Trash class="w-4 cursor-pointer" onclick={() => onDelete?.()}></Trash>
				<Edit class="w-4 cursor-pointer"></Edit>
			</div>
		</Card.Title>
		<Card.Description>Modified {new Date(modifiedDate).toLocaleString()}</Card.Description>
	</Card.Header>
	<Card.Content class="flex flex-col gap-4"></Card.Content>
</Card.Root>
