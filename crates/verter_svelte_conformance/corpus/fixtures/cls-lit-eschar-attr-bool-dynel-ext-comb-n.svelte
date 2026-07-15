<script>
	let tag = $state('div');
</script>

<div class="wrap">
	<svelte:element this={tag} data-x>x</svelte:element>
</div>

<style>
	.wrap .\zz {
		color: red;
	}
</style>
