<script>
	let count = $state(0);
	let doubled = $derived(count * 2);

	$effect(() => {
		console.log(doubled);
	});
</script>

<button onclick={() => count++}>{count} / {doubled}</button>
