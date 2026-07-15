<script>
	let tag = $state('div');
	let { rest } = $props();
</script>

<svelte:element this={tag} {...rest}>x</svelte:element>

<style>
	div {
		color: red;
	}
</style>
