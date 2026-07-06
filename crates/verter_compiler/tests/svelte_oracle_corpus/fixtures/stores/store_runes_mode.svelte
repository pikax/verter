<script>
	import { writable } from 'svelte/store';

	const c = writable(0);
	let n = $state(0);
</script>

<p>{$c}</p>
<button onclick={() => n++}>{n}</button>
