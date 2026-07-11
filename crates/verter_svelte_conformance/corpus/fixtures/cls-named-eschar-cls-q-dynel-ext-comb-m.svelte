<script>
	let tag = $state('div');
</script>

<div class="wrap">
	<svelte:element this={tag} class="a a&amp;b">x</svelte:element>
</div>

<style>
	.wrap .a\&b {
		color: red;
	}
</style>
