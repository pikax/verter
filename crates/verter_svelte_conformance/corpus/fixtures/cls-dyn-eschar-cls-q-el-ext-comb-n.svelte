<script>
	let { flag } = $props();
</script>

<div class="wrap">
	<div class="{flag ? 'a a&b' : 'a\u0026b'}">x</div>
</div>

<style>
	.wrap .\zz {
		color: red;
	}
</style>
