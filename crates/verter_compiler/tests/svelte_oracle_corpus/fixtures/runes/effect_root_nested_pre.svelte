<script>
	let c = $state(0);
	const stop = $effect.root(() => {
		$effect.pre(() => console.log(c));
		return () => {};
	});
</script>
<button onclick={() => c++}>{c}</button>
