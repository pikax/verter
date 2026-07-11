<script>
	import Child from './Child.svelte';
	let { flag } = $props();
</script>

<Child>
	<div class="{flag ? 'a a&b&c' : 'a\u0026b\u0026c'}">x</div>
</Child>

<style>
	:global(.a\26 b\&c) {
		color: red;
	}
</style>
