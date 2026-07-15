<svelte:options css="injected" />

<script>
	let tag = $state('div');
	let { rest } = $props();
</script>

<div class="wrap">
	<svelte:element this={tag} {...rest}>x</svelte:element>
</div>

<style>
	.wrap [data-x="b"] {
		color: red;
	}
</style>
