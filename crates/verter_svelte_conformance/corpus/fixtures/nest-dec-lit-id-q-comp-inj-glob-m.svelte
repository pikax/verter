<svelte:options css="injected" />

<script>
	import Child from './Child.svelte';
</script>

<Child>
	<div id="&#98;">x</div>
</Child>

<style>
	:global(&) {
		color: red;
	}
</style>
