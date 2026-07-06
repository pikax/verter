<svelte:options runes={true} />

<script>
	import { writable } from 'svelte/store';

	const state = writable(0);
</script>

<p>{$state}</p>
