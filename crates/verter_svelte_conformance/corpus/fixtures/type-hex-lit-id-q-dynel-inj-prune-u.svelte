<svelte:options css="injected" />

<script>
	let tag = $state('div');
</script>

<svelte:element this={tag} id="&#x62;">x</svelte:element>

<style>
	div {
		color: red;
	}

	.unused-prune {
		color: blue;
	}
</style>
