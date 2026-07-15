<script>
	import { writable } from 'svelte/store';

	const a = writable(1);
	const b = writable(2);
</script>

<p>{$a}</p>
<p>{$b}</p>
