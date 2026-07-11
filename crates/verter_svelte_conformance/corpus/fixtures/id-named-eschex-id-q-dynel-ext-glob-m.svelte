<script>
	let tag = $state('div');
</script>

<svelte:element this={tag} id="a&amp;b">x</svelte:element>

<style>
	:global(#a\26 b) {
		color: red;
	}
</style>
