<script>
	let { rest } = $props();
</script>

<div {...rest}>x</div>

<style>
	[data-x="a\&b"] {
		color: red;
	}

	.unused-prune {
		color: blue;
	}
</style>
