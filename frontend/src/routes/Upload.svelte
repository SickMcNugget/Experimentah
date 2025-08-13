<script lang="ts">
	import { Button } from '$lib/components/ui/button/index.js';
	import { Label } from '$lib/components/ui/label/index.js';
	import { Input } from '$lib/components/ui/input/index.js';
	import * as Card from '$lib/components/ui/card/index.js';
	import { Upload } from '@lucide/svelte';

	let { configType, onUpload }: { configType: 'infrastructure' | 'experiment'; onUpload?: (file: File) => void } = $props();
</script>

<Card.Root class="w-full">
	<Card.Header>
		<Card.Title>Upload <span class="capitalize">{configType}</span> Configuration</Card.Title>
	</Card.Header>
	<Card.Content>
		<div
			class="flex flex-col gap-2 items-center justify-center h-[10rem] rounded-sm border-2 border-dashed border-secondary p-4"
		>
			<Upload class="text-muted-foreground"></Upload>
			<p class="text-muted-foreground text-sm">
				Drop your configuration file here or click to browse
			</p>
			<label
				class="text-sm text-muted-foreground cursor-pointer border-2 px-3 py-2 rounded-md border-secondary"
			>
				Select File
				<input
					type="file"
					accept=".toml"
					class="hidden"
					onchange={(e) => {
						const input = e.currentTarget as HTMLInputElement;
						const file = input.files?.[0];
						if (file) onUpload?.(file);
						input.value = '';
					}}
				/>
			</label>
		</div>
	</Card.Content>
</Card.Root>
