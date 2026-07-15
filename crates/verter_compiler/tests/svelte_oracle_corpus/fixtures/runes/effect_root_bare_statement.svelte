<script>
	let x = $state(0);
	$effect.root(() => {
		console.log(x);
		return () => {};
	});
</script>

<button onclick={() => x++}>{x}</button>
