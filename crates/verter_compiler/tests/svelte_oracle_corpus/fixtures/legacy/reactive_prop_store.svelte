<script>
	import { writable } from 'svelte/store';
	export let p;
	const c = writable(0);
	$: y = p + $c + 1;
</script>

<p>{y}</p>
