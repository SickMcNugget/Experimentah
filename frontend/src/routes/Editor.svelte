<script lang="ts">
	import { EditorView, basicSetup } from 'codemirror';
	import { StreamLanguage } from '@codemirror/language';
	import { toml } from '@codemirror/legacy-modes/mode/toml';
	import { Annotation } from '@codemirror/state';
	import { untrack } from 'svelte';
	import { cn } from '$lib/components/utils';

	let { text = $bindable(), class: className = '' } = $props<{ text: string; class?: string }>();

	const externalUpdate = Annotation.define<boolean>();

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
		{ dark: true }
	);

	let editorContainer: HTMLDivElement;
	let editorView: EditorView | null = null;

	// Setup editor once when container is available
	$effect(() => {
		if (editorContainer && !editorView) {
			editorView = new EditorView({
				// 2. Use untrack to prevent a reactive dependency
				doc: untrack(() => text),
				extensions: [
					basicSetup,
					StreamLanguage.define(toml),
					editorTheme,
					EditorView.updateListener.of((update) => {
						if (update.docChanged) {
							const isExternal = update.transactions.some((tr) => tr.annotation(externalUpdate));
							if (!isExternal) {
								text = update.state.doc.toString();
							}
						}
					})
				],
				parent: editorContainer
			});

			// Cleanup on component destroy
			return () => {
				if (editorView) {
					editorView.destroy();
					editorView = null;
				}
			};
		}
	});

	// Sync editor content with text prop when it changes
	$effect(() => {
		if (editorView && editorView.state.doc.toString() !== text) {
			editorView.dispatch({
				changes: { from: 0, to: editorView.state.doc.length, insert: text },
				annotations: [externalUpdate.of(true)]
			});
		}
	});
</script>

<div
	bind:this={editorContainer}
	class={cn('flex-grow relative overflow-y-auto h-full', className)}
></div>
