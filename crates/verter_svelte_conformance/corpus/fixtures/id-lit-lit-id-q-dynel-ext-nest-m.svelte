<script>
	let tag = $state('div');
</script>

<svelte:element this={tag} id="b">x</svelte:element>

<style>
	#b {
		color: red;

		&:hover {
			color: blue;
		}
	}
</style>
