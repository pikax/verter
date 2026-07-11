<svelte:options css="injected" />

<script>
	import Child from './Child.svelte';
</script>

<Child>
	<div class="a a&#38;&#x62;">x</div>
</Child>

<style>
	:global(.a\26 b) {
		color: red;
	}
</style>
