<script>
	let { flag } = $props();
</script>

<div data-x={flag ? 'b' : '\u0062'}>x</div>

<style>
	.zz {
		color: red;
	}
</style>
