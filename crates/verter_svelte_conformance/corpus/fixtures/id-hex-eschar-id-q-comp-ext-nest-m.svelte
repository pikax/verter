<script>
	import Child from './Child.svelte';
</script>

<Child>
	<div id="a&#x26;b">x</div>
</Child>

<style>
	#a\&b {
		color: red;

		&:hover {
			color: blue;
		}
	}
</style>
