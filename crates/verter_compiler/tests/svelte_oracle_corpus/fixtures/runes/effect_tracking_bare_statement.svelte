<script>
	let x = $state(0);
	$effect.tracking();
</script>

<button onclick={() => x++}>{x}</button>
