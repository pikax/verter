<script>
	let tag = $state('div');
</script>

<svelte:element this={tag} id="a&#38;b">x</svelte:element>

<style>
	[data-x="\7a z"] {
		color: red;

		&:hover {
			color: blue;
		}
	}
</style>
