<script>
	let { flag } = $props();
</script>

<div id={flag ? 'b' : '\u0062'}>x</div>

<style>
	#b {
		color: red;
	}
</style>
