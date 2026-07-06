<script>
	import { writable, derived } from 'svelte/store';

	const a = writable(1);
	const doubled = derived(a, ($a) => $a * 2);
</script>

<p>{$doubled}</p>
