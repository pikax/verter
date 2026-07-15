<script>
	let { rest } = $props();
</script>

<slot>
	<div class="wrap">
	<div {...rest}>x</div>
</div>
</slot>

<style>
	.wrap * {
		color: red;
	}
</style>
