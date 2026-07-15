<script>
	let { rest } = $props();
</script>

<div {...rest}>x</div>

<style>
	[data-x="a\26 b\&c"] {
		color: red;
	}

	.unused-prune {
		color: blue;
	}
</style>
