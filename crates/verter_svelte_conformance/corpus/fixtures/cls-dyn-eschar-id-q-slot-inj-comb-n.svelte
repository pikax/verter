<svelte:options css="injected" />

<script>
	let { flag } = $props();
</script>

<slot>
	<div class="wrap">
	<div id="{flag ? 'a&b' : 'a\u0026b'}">x</div>
</div>
</slot>

<style>
	.wrap .\zz {
		color: red;
	}
</style>
