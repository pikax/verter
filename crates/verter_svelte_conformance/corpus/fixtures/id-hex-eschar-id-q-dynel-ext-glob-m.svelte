<script>
	let tag = $state('div');
</script>

<svelte:element this={tag} id="a&#x26;b">x</svelte:element>

<style>
	:global(#a\&b) {
		color: red;
	}
</style>
