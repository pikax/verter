<script>
	let tag = $state('div');
</script>

<svelte:element this={tag} id="a&#x26;b">x</svelte:element>

<style>
	#a\26 b {
		color: red;

		&:hover {
			color: blue;
		}
	}
</style>
