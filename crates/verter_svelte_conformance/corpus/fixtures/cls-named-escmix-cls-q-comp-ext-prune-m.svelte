<script>
	import Child from './Child.svelte';
</script>

<Child>
	<div class="a a&amp;b&amp;c">x</div>
</Child>

<style>
	.a\26 b\&c {
		color: red;
	}

	.unused-prune {
		color: blue;
	}
</style>
