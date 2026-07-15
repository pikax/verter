<script>
	let count = $state(0);
</script>

<svelte:window onresize={() => count++} />
<p>hi</p>
