<script>
	let tag = $state('div');
	let { rest } = $props();
</script>

<svelte:element this={tag} {...rest}>x</svelte:element>

<style>
	[data-x="b"] {
		color: red;
	}

	.unused-prune {
		color: blue;
	}
</style>
