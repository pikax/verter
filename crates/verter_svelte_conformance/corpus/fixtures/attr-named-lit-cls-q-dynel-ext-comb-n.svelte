<script>
	let tag = $state('div');
</script>

<div class="wrap">
	<svelte:element this={tag} class="a a&copy;b">x</svelte:element>
</div>

<style>
	.wrap [data-x="zz"] {
		color: red;
	}
</style>
