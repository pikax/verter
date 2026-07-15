<script>
	let tag = $state('div');
</script>

<svelte:element this={tag} data-x="a&#38;&#x62;">x</svelte:element>

<style>
	[data-x="\zz"] {
		color: red;
	}

	.unused-prune {
		color: blue;
	}
</style>
