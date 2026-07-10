<script>
	let tag = $state('div');
	let { rest } = $props();
</script>

<svelte:element this={tag} {...rest}>content</svelte:element>

<style>
	div {
		color: blue;
	}
</style>
