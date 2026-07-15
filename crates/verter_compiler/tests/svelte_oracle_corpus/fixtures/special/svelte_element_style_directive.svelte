<script>
	let tag = $state('div');
	let c = $state('red');
</script>

<svelte:element this={tag} class="x" style:color={c}>hi</svelte:element>
