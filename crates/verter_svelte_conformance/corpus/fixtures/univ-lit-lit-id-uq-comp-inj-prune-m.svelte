<svelte:options css="injected" />

<script>
	import Child from './Child.svelte';
</script>

<Child>
	<div id=b>x</div>
</Child>

<style>
	* {
		color: red;
	}

	.unused-prune {
		color: blue;
	}
</style>
