<script>
	let tag = $state('div');
</script>

<svelte:element this={tag} data-x="&#98;">x</svelte:element>

<style>
	div {
		color: red;

		&:hover {
			color: blue;
		}
	}
</style>
