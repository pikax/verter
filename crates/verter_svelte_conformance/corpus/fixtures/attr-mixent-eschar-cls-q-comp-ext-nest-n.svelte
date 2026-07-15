<script>
	import Child from './Child.svelte';
</script>

<Child>
	<div class="a a&#38;&#x62;">x</div>
</Child>

<style>
	[data-x="\zz"] {
		color: red;

		&:hover {
			color: blue;
		}
	}
</style>
