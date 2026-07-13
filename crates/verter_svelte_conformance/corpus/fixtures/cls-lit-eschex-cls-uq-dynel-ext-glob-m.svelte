<script>
	let tag = $state('div');
</script>

<svelte:element this={tag} class=a&b>x</svelte:element>

<style>
	:global(.a\26 b) {
		color: red;
	}
</style>
