<script>
	let tag = $state('div');
</script>

<svelte:element this={tag} class=a&b&c>x</svelte:element>

<style>
	.a\26 b\&c {
		color: red;

		&:hover {
			color: blue;
		}
	}
</style>
