<script>
	let tag = $state('div');
</script>

<svelte:element this={tag} data-x=a&#x26;b>x</svelte:element>

<style>
	[data-x="\zz"] {
		color: red;
	}

	.unused-prune {
		color: blue;
	}
</style>
