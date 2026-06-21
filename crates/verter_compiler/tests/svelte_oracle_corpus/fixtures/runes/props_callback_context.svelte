<script>
	let { cb } = $props();
	let count = $state(0);
	cb();
</script>

<button onclick={() => count++}>{count}</button>
