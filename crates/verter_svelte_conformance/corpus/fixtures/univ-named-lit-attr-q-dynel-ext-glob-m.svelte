<script>
	let tag = $state('div');
</script>

<svelte:element this={tag} data-x="a&copy;b">x</svelte:element>

<style>
	:global(*) {
		color: red;
	}
</style>
