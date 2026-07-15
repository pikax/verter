<svelte:options css="injected" />

<script>
	import Child from './Child.svelte';
	let { value } = $props();
</script>

<Child>
	<div class={value}>x</div>
</Child>

<style>
	.a\&b {
		color: red;
	}
</style>
