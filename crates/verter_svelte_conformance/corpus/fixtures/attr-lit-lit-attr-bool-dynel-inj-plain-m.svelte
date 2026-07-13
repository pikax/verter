<svelte:options css="injected" />

<script>
	let tag = $state('div');
</script>

<svelte:element this={tag} data-x>x</svelte:element>

<style>
	[data-x] {
		color: red;
	}
</style>
