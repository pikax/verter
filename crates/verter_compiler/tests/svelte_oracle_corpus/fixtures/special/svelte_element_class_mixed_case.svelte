<script>
	let tag = $state('div');
	let x = $state(false);
</script>

<svelte:element this={tag} CLASS="card" class:active={x}>hi</svelte:element>
