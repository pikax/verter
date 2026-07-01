<script>
	let tag = $state('div');
	let x = $state(false);
</script>

<svelte:element this={tag} class="card" class:active={x}>hi</svelte:element>
