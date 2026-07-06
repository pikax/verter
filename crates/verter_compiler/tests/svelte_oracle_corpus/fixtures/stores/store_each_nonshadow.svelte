<script>
	import { writable } from 'svelte/store';

	const y = writable(1);
	let { items } = $props();
</script>

{#each items as x}<p>{$y}</p>{/each}
