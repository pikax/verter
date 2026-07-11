<script>
	let { rest } = $props();
</script>

<div {...rest}>x</div>

<style>
	[data-x="a\26 b"] {
		color: red;
	}
</style>
