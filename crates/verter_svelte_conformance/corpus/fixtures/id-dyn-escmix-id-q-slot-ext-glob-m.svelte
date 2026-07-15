<script>
	let { flag } = $props();
</script>

<slot>
	<div id="{flag ? 'a&b&c' : 'a\u0026b\u0026c'}">x</div>
</slot>

<style>
	:global(#a\26 b\&c) {
		color: red;
	}
</style>
