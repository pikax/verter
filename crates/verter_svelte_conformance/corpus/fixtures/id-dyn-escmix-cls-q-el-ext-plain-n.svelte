<script>
	let { flag } = $props();
</script>

<div class="{flag ? 'a a&b&c' : 'a\u0026b\u0026c'}">x</div>

<style>
	#\7a \z {
		color: red;
	}
</style>
