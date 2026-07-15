<script>
	import { getContext } from 'svelte';
	let count = $state(0);
	const theme = getContext('theme');
</script>
<button onclick={() => count++}>{count}</button>
