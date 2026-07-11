<script>
	let tag = $state('div');
</script>

<div class="wrap">
	<svelte:element this={tag} class="a a&amp;b&amp;c">x</svelte:element>
</div>

<style>
	.wrap [data-x="\7a \z"] {
		color: red;
	}
</style>
