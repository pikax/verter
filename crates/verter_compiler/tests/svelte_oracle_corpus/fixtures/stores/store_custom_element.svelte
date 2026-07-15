<svelte:options customElement="my-el" />

<script>
	import { writable } from 'svelte/store';

	const c = writable(0);
	let { label } = $props();
</script>

<p>{$c}{label}</p>
