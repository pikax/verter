<script>
	let { value } = $props();
</script>

<div data-x={value}>x</div>

<style>
	[data-x="a\26 b"] {
		color: red;
	}

	.unused-prune {
		color: blue;
	}
</style>
