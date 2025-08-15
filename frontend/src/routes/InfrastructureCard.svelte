<script lang="ts">
	// Component Imports
	import * as Card from '$lib/components/ui/card/index.js';
	import * as Dialog from '$lib/components/ui/dialog/index.js';
	import { Button } from '$lib/components/ui/button/index.js';
	import { Trash, Edit } from '@lucide/svelte';
	import Editor from './Editor.svelte';
	import { handleModify } from './configs.svelte';

	// State for the main TOML content
	let {
		name,
		modifiedDate,
		content,
		onDelete
	}: { name: string; modifiedDate: string; content?: string; onDelete?: () => void } = $props();

	// State for the dialog and its editor
	let open = $state(false);
	let editorContent = $state(content ?? '');

	const openDialog = () => {
		// Load the current TOML into the editor's state when opening
		editorContent = content ?? '';
		open = true;
	};

	const handleSave = () => {
		handleModify(name, editorContent, 'infrastructure');
		open = false;
	};

	const handleCancel = () => {
		open = false;
	};
</script>

<Card.Root>
	<Card.Header>
		<Card.Title class="relative">
			{name}
			<div class="absolute flex gap-4 right-0 -top-1">
				<Trash class="w-4 cursor-pointer" onclick={() => onDelete?.()} />
				<Edit class="w-4 cursor-pointer" onclick={openDialog} />
			</div>
		</Card.Title>
		<Card.Description>Modified {new Date(modifiedDate).toLocaleString()}</Card.Description>
	</Card.Header>
</Card.Root>

<Dialog.Root bind:open>
	<Dialog.Content class="h-[80vh] w-full min-w-[calc(100vw-2rem)] flex flex-col gap-4">
		<Dialog.Header>
			<Dialog.Title>Edit {name}</Dialog.Title>
			<Dialog.Description>
				Make changes to your configuration file here. Click save when you're done.
			</Dialog.Description>
		</Dialog.Header>

		<Editor bind:text={editorContent} class="flex-grow" />

		<Dialog.Footer>
			<Button variant="outline" onclick={handleCancel}>Cancel</Button>
			<Button onclick={handleSave}>Save</Button>
		</Dialog.Footer>
	</Dialog.Content>
</Dialog.Root>
