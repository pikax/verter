<svelte:options css="injected" />

<script>
	import Child from './Child.svelte';
</script>

<Child>
	<div id="a&#x26;b">x</div>
</Child>

<style>
	#a\26 b {
		color: red;
	}
</style>
