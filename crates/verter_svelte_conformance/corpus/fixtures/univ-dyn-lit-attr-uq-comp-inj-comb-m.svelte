<svelte:options css="injected" />

<script>
	import Child from './Child.svelte';
	let { flag } = $props();
</script>

<Child>
	<div class="wrap">
	<div data-x={flag ? 'b' : '\u0062'}>x</div>
</div>
</Child>

<style>
	.wrap * {
		color: red;
	}
</style>
