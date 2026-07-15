<script>
	let { flag } = $props();
</script>

<div class="{flag ? 'a a&b' : 'a\u0026b'}">x</div>

<style>
	:global(.a\26 b) {
		color: red;
	}
</style>
