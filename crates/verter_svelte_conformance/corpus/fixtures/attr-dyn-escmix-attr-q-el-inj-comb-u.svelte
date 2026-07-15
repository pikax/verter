<svelte:options css="injected" />

<script>
	let { value } = $props();
</script>

<div class="wrap">
	<div data-x="{value}">x</div>
</div>

<style>
	.wrap [data-x="a\26 b\&c"] {
		color: red;
	}
</style>
