<script>
	let { flag } = $props();
</script>

<div class={flag ? 'a b' : '\u0062'}>x</div>

<style>
	.zz {
		color: red;
	}
</style>
