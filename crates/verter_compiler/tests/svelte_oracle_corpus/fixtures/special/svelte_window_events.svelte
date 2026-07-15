<script>
	let n = $state(0);
</script>

<svelte:window onkeydown={() => n++} onresize={() => n += 1} />
