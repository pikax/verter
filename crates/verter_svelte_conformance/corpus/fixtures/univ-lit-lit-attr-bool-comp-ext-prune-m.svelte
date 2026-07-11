<script>
	import Child from './Child.svelte';
</script>

<Child>
	<div data-x>x</div>
</Child>

<style>
	* {
		color: red;
	}

	.unused-prune {
		color: blue;
	}
</style>
