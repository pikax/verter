<script>
	let tag = $state('div');
</script>

<svelte:element this={tag} data-x>x</svelte:element>

<style>
	div {
		color: red;
	}

	.unused-prune {
		color: blue;
	}
</style>
