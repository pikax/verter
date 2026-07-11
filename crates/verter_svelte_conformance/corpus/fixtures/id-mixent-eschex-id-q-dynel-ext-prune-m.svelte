<script>
	let tag = $state('div');
</script>

<svelte:element this={tag} id="a&#38;&#x62;">x</svelte:element>

<style>
	#a\26 b {
		color: red;
	}

	.unused-prune {
		color: blue;
	}
</style>
