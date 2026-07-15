<script>
	let { value } = $props();
</script>

<slot>
	<div data-x={value}>x</div>
</slot>

<style>
	[data-x="b"] {
		color: red;
	}
</style>
