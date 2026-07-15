<svelte:options css="injected" />

<script>
	let tag = $state('div');
</script>

<svelte:element this={tag} id=a&amp;b>x</svelte:element>

<style>
	#\7a z {
		color: red;
	}

	.unused-prune {
		color: blue;
	}
</style>
