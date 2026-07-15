<svelte:options css="injected" />

<script>
	import Child from './Child.svelte';
	let { flag } = $props();
</script>

<Child>
	<div class="wrap">
	<div id={flag ? 'a&b&c' : 'a\u0026b\u0026c'}>x</div>
</div>
</Child>

<style>
	.wrap #a\26 b\&c {
		color: red;
	}
</style>
