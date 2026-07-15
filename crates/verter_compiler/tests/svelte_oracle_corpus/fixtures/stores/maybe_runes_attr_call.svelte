<script>
	import { writable } from 'svelte/store';
	const s = writable({});
</script>

<div title={$s.m()}></div>
