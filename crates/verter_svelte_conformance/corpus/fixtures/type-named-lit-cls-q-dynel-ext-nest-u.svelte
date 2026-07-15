<script>
	let tag = $state('div');
</script>

<svelte:element this={tag} class="a a&copy;b">x</svelte:element>

<style>
	div {
		color: red;

		&:hover {
			color: blue;
		}
	}
</style>
