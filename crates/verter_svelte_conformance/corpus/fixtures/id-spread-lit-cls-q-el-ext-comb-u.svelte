<script>
	let { rest } = $props();
</script>

<div class="wrap">
	<div {...rest}>x</div>
</div>

<style>
	.wrap #b {
		color: red;
	}
</style>
