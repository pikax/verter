<script>
	import Child from './Child.svelte';
</script>

<Child>
	<div class="a a&#38;b">x</div>
</Child>

<style>
	[data-x="\zz"] {
		color: red;
	}

	.unused-prune {
		color: blue;
	}
</style>
