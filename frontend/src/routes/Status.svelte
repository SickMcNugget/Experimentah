<script lang="ts">
	import { Button } from '$lib/components/ui/button/index.js';
	import * as Card from '$lib/components/ui/card/index.js';
	import { Badge } from '$lib/components/ui/badge/index.js';
	import { Square } from '@lucide/svelte';
</script>

<div>
	<Card.Root class="w-full">
		<Card.Header>
			<Card.Title
				><span class="flex gap-2 items-center">
					<div class="w-4 h-4 bg-green-600 rounded-full"></div>
					Beans.toml
				</span></Card.Title
			>
			<Card.Description>Running for 5m 20s</Card.Description>
			<Card.Action>
				<Button variant="destructive">
					Stop
					<Square></Square>
				</Button>
			</Card.Action>
		</Card.Header>
	</Card.Root>
</div>
