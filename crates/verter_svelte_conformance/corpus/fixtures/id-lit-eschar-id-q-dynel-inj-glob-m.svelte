<svelte:options css="injected" />

<script>
	let tag = $state('div');
</script>

<svelte:element this={tag} id="a&b">x</svelte:element>

<style>
	:global(#a\&b) {
		color: red;
	}
</style>
