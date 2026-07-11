<script>
	let { rest } = $props();
</script>

<div {...rest}>x</div>

<style>
	[data-x="b"] {
		& {
			color: red;
		}
	}
</style>
