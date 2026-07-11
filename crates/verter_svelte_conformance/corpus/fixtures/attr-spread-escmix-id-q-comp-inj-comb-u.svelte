<svelte:options css="injected" />

<script>
	import Child from './Child.svelte';
	let { rest } = $props();
</script>

<Child>
	<div class="wrap">
	<div {...rest}>x</div>
</div>
</Child>

<style>
	.wrap [data-x="a\26 b\&c"] {
		color: red;
	}
</style>
