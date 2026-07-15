<script>
	let tag = $state('div');
</script>

<svelte:element this={tag} class="a&#x20;b">x</svelte:element>

<style>
	* {
		color: red;
	}

	.unused-prune {
		color: blue;
	}
</style>
