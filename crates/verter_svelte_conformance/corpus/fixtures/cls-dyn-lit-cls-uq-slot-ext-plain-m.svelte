<script>
	let { flag } = $props();
</script>

<slot>
	<div class={flag ? 'a b' : '\u0062'}>x</div>
</slot>

<style>
	.b {
		color: red;
	}
</style>
