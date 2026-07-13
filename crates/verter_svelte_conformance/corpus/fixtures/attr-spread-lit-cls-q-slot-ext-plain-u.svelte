<script>
	let { rest } = $props();
</script>

<slot>
	<div {...rest}>x</div>
</slot>

<style>
	[data-x="b"] {
		color: red;
	}
</style>
