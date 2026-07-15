<svelte:options css="injected" />

<script>
	let { rest } = $props();
</script>

<slot>
	<div {...rest}>x</div>
</slot>

<style>
	#a\&b {
		color: red;

		&:hover {
			color: blue;
		}
	}
</style>
