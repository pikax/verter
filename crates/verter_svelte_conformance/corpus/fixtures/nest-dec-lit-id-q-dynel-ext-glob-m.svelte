<script>
	let tag = $state('div');
</script>

<svelte:element this={tag} id="&#98;">x</svelte:element>

<style>
	:global(&) {
		color: red;
	}
</style>
