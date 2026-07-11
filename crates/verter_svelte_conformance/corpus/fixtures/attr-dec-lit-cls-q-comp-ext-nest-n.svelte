<script>
	import Child from './Child.svelte';
</script>

<Child>
	<div class="a&#32;b">x</div>
</Child>

<style>
	[data-x="zz"] {
		color: red;

		&:hover {
			color: blue;
		}
	}
</style>
