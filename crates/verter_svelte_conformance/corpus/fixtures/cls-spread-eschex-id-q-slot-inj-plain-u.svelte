<svelte:options css="injected" />

<script>
	let { rest } = $props();
</script>

<slot>
	<div {...rest}>x</div>
</slot>

<style>
	.a\26 b {
		color: red;
	}
</style>
