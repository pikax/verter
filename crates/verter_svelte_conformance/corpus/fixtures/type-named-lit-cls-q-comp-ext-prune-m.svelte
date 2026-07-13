<script>
	import Child from './Child.svelte';
</script>

<Child>
	<div class="a a&copy;b">x</div>
</Child>

<style>
	div {
		color: red;
	}

	.unused-prune {
		color: blue;
	}
</style>
