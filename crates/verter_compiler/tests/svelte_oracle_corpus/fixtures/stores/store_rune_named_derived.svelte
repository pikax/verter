<script>
	import { writable } from 'svelte/store';

	const derived = writable(0);
</script>

<p>{$derived}</p>
