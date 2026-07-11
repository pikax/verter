<script>
	let tag = $state('div');
</script>

<div class="wrap">
	<svelte:element this={tag} class="a&#x20;b">x</svelte:element>
</div>

<style>
	.wrap div {
		color: red;
	}
</style>
