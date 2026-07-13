<script>
	let tag = $state('div');
</script>

<svelte:element this={tag} class=a&#38;b&#38;c>x</svelte:element>

<style>
	[data-x="\7a \z"] {
		color: red;

		&:hover {
			color: blue;
		}
	}
</style>
