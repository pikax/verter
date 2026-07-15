<script>
	let { rest } = $props();
</script>

<slot>
	<div {...rest}>x</div>
</slot>

<style>
	:global(div) {
		color: red;
	}
</style>
