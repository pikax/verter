<script>
	let { rest } = $props();
</script>

<slot>
	<div {...rest}>x</div>
</slot>

<style>
	div {
		color: red;
	}
</style>
