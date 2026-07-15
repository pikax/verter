<script>
	let tag = $state('div');
	let w = $state(0);
</script>

<svelte:element this={tag} bind:clientWidth={w}>hi</svelte:element>
