<script>
	let { rest } = $props();
</script>

<slot>
	<div {...rest}>x</div>
</slot>

<style>
	#a\26 b\&c {
		color: red;
	}

	.unused-prune {
		color: blue;
	}
</style>
