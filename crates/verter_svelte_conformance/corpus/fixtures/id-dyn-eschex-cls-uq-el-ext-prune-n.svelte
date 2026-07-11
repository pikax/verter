<script>
	let { flag } = $props();
</script>

<div class={flag ? 'a a&b' : 'a\u0026b'}>x</div>

<style>
	#\7a z {
		color: red;
	}

	.unused-prune {
		color: blue;
	}
</style>
