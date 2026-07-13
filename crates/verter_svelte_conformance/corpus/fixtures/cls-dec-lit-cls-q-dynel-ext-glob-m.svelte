<script>
	let tag = $state('div');
</script>

<svelte:element this={tag} class="a&#32;b">x</svelte:element>

<style>
	:global(.b) {
		color: red;
	}
</style>
