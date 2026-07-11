<svelte:options css="injected" />

<script>
	let tag = $state('div');
	let { value } = $props();
</script>

<div class="wrap">
	<svelte:element this={tag} data-x="{value}">x</svelte:element>
</div>

<style>
	.wrap [data-x="a\26 b"] {
		color: red;
	}
</style>
