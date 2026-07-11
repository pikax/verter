<script>
	let tag = $state('div');
</script>

<div class="wrap">
	<svelte:element this={tag} class="a&#x20;b">x</svelte:element>
</div>

<style>
	.wrap .b {
		color: red;
	}
</style>
