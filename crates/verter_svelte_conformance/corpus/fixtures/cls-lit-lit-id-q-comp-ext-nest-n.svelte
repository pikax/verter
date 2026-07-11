<script>
	import Child from './Child.svelte';
</script>

<Child>
	<div id="b">x</div>
</Child>

<style>
	.zz {
		color: red;

		&:hover {
			color: blue;
		}
	}
</style>
