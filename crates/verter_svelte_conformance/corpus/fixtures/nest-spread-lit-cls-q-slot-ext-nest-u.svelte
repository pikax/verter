<script>
	let { rest } = $props();
</script>

<slot>
	<div {...rest}>x</div>
</slot>

<style>
	.b {
		& {
			color: red;
		}
	}
</style>
