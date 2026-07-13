<script>
	let tag = $state('div');
</script>

<svelte:element this={tag} id="a&copy;b">x</svelte:element>

<style>
	#a©b {
		color: red;
	}
</style>
