<script>
	let { flag } = $props();
</script>

<slot>
	<div id={flag ? 'b' : '\u0062'}>x</div>
</slot>

<style>
	.zz {
		color: red;
	}
</style>
