<svelte:options css="injected" />

<script>
	import Child from './Child.svelte';
</script>

<Child>
	<div id="a&amp;b">x</div>
</Child>

<style>
	#a\&b {
		color: red;
	}
</style>
