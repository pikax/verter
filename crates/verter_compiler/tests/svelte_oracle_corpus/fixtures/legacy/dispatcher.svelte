<script>
	import { createEventDispatcher } from 'svelte';
	const dispatch = createEventDispatcher();
	function fire() { dispatch('go', 1); }
</script>

<button onclick={fire}>go</button>
