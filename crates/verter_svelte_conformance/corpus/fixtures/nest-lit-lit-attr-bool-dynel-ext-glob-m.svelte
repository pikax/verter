<script>
	let tag = $state('div');
</script>

<svelte:element this={tag} data-x>x</svelte:element>

<style>
	:global(&) {
		color: red;
	}
</style>
