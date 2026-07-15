<script>
	let tag = $state('div');
	let { rest } = $props();
</script>

<div class="wrap">
	<svelte:element this={tag} {...rest}>x</svelte:element>
</div>

<style>
	.wrap #a\26 b\&c {
		color: red;
	}
</style>
