<script>
	import { writable } from 'svelte/store';

	const count = writable(0);

	function inc() {
		count.update((n) => n + 1);
	}
</script>

<button onclick={inc}>{$count}</button>
