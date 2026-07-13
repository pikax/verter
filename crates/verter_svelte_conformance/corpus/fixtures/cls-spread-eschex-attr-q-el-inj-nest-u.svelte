<svelte:options css="injected" />

<script>
	let { rest } = $props();
</script>

<div {...rest}>x</div>

<style>
	.a\26 b {
		color: red;

		&:hover {
			color: blue;
		}
	}
</style>
