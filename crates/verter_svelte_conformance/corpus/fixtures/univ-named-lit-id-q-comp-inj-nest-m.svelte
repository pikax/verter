<svelte:options css="injected" />

<script>
	import Child from './Child.svelte';
</script>

<Child>
	<div id="a&copy;b">x</div>
</Child>

<style>
	* {
		color: red;

		&:hover {
			color: blue;
		}
	}
</style>
