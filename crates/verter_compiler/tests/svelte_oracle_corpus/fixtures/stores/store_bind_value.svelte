<script>
	import { writable } from 'svelte/store';

	const c = writable('');
</script>

<input bind:value={$c} />
