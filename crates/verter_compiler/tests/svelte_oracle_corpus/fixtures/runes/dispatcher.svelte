<script>
	import { createEventDispatcher } from 'svelte';
	let n = $state(0);
	const dispatch = createEventDispatcher();
	function fire() { dispatch('go', n); }
</script>

<button onclick={fire}>go</button>
