<script>
	import Child from './Child.svelte';
</script>

<Child>
	<div data-x=b>x</div>
</Child>

<style>
	[data-x="b"] {
		color: red;
	}
</style>
