<script>
	let tag = $state('div');
</script>

<svelte:element this={tag} data-x=b>x</svelte:element>

<style>
	[data-x="b"] {
		color: red;
	}
</style>
