<script>
	let tag = $state('div');
	let { value } = $props();
</script>

<svelte:element this={tag} id={value}>x</svelte:element>

<style>
	#a\&b {
		color: red;
	}

	.unused-prune {
		color: blue;
	}
</style>
