<svelte:options css="injected" />

<script>
	import Child from './Child.svelte';
	let { rest } = $props();
</script>

<Child>
	<div {...rest}>x</div>
</Child>

<style>
	[data-x="a\26 b"] {
		color: red;
	}
</style>
