<script>
	import Child from './Child.svelte';
</script>

<Child>
	<div data-x>x</div>
</Child>

<style>
	#zz {
		color: red;
	}

	.unused-prune {
		color: blue;
	}
</style>
