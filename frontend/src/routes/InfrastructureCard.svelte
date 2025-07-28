<script lang="ts">
	import * as Card from '$lib/components/ui/card/index.js';
	import { Trash, Edit } from '@lucide/svelte';
	import * as Dialog from '$lib/components/ui/dialog/index.js';
	import { Button } from '$lib/components/ui/button/index.js';
	import { EditorView, basicSetup } from 'codemirror';
	import { StreamLanguage } from '@codemirror/language';
	import { toml } from '@codemirror/legacy-modes/mode/toml';

	const editorTheme = EditorView.theme(
		{
			'&': {
				backgroundColor: '#282c34',
				color: '#abb2bf',
				height: '100%',
				borderRadius: '0.375rem'
			},
			'.cm-gutters': {
				backgroundColor: '#21252b',
				color: '#6c757d',
				borderRight: '1px solid #323843'
			},
			'.cm-activeLine': {
				backgroundColor: '#2c313a'
			},
			'.cm-activeLineGutter': {
				backgroundColor: '#2c313a'
			},
			'.cm-selectionMatch': {
				backgroundColor: '#3c424d'
			},
			// The cursor
			'.cm-cursor': {
				borderLeftColor: '#528bff'
			},
			'.ͼe': { color: '#d2a7fc' },
			'.ͼm': { color: 'grey' },
			'.ͼd': { color: '#53eda8' },
			'.ͼc': { color: '#eb8621' }
		},
		{ dark: true } // Mark this as a dark theme
	);

	let exampleToml = $state(`[[Test]]
name = "localhost-experiment"
description = "Testing the functionality of the software completely using localhost"
kind = "localhost-result"
execute = "./scripts/actual-work.sh"
arguments = 1
runners = ["runner1"]
# Optional, defaults to 1
runs = 1
# Let's add more lines to demonstrate scrolling
# Line 1
# Line 2
# Line 3
# Line 4
# Line 5
# Line 6
# Line 7
# Line 8
# Line 9
# Line 10
# Line 11
# Line 12
# Line 13
# Line 14
# Line 15
# Line 16
# Line 17
# Line 18
# Line 19
# End of file
`);

	let open = $state(false);
	let editorContainer: HTMLDivElement;
	let editorView: EditorView;
	let editorContent = $state(exampleToml);

	const openDialog = () => {
		editorContent = exampleToml;
		open = true;
	};

	const handleSave = () => {
		exampleToml = editorView.state.doc.toString();
		open = false;
	};

	const handleCancel = () => {
		open = false;
	};

	$effect(() => {
		if (open && editorContainer) {
			if (editorView) {
				editorView.destroy();
			}
			editorView = new EditorView({
				doc: editorContent,
				extensions: [basicSetup, StreamLanguage.define(toml), editorTheme],
				parent: editorContainer
			});
		}
	});
</script>

<Card.Root>
	<Card.Header>
		<Card.Title class="relative">
			Beans.toml
			<div class="absolute flex gap-4 right-0 -top-1">
				<Trash class="w-4 cursor-pointer" />
				<Edit class="w-4 cursor-pointer" onclick={openDialog} />
			</div>
		</Card.Title>
		<Card.Description>Modified 01/07/2025 01:00 PM</Card.Description>
	</Card.Header>
</Card.Root>

<Dialog.Root bind:open>
	<Dialog.Content class="h-[80vh] w-full min-w-[75rem] flex flex-col gap-4">
		<Dialog.Header>
			<Dialog.Title>Edit Beans.toml</Dialog.Title>
			<Dialog.Description>
				Make changes to your configuration file here. Click save when you're done.
			</Dialog.Description>
		</Dialog.Header>

		<div bind:this={editorContainer} class="flex-grow relative overflow-y-auto"></div>

		<Dialog.Footer>
			<Button variant="outline" onclick={handleCancel}>Cancel</Button>
			<Button onclick={handleSave}>Save</Button>
		</Dialog.Footer>
	</Dialog.Content>
</Dialog.Root>
