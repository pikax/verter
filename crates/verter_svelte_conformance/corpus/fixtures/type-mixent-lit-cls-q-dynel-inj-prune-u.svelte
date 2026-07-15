<svelte:options css="injected" />

<script>
	let tag = $state('div');
</script>

<svelte:element this={tag} class="a &#98;&#x62;">x</svelte:element>

<style>
	div {
		color: red;
	}

	.unused-prune {
		color: blue;
	}
</style>
