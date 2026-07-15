<script>
	let tag = $state('div');
	let w = $state(1);
	let el = $state(null);
</script>

<svelte:element this={tag} data-x={w} bind:this={el}>hi</svelte:element>
