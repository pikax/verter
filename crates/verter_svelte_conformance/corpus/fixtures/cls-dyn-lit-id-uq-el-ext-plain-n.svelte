<script>
	let { flag } = $props();
</script>

<div id={flag ? 'b' : '\u0062'}>x</div>

<style>
	.zz {
		color: red;
	}
</style>
