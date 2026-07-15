<script>
	let { flag } = $props();
</script>

<slot>
	<div data-x={flag ? 'b' : '\u0062'}>x</div>
</slot>

<style>
	[data-x="b"] {
		color: red;
	}
</style>
