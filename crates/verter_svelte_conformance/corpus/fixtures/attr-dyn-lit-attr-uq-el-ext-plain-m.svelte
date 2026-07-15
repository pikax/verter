<script>
	let { flag } = $props();
</script>

<div data-x={flag ? 'b' : '\u0062'}>x</div>

<style>
	[data-x="b"] {
		color: red;
	}
</style>
