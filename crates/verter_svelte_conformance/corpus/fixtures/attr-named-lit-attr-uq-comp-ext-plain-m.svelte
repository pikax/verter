<script>
	import Child from './Child.svelte';
</script>

<Child>
	<div data-x=a&copy;b>x</div>
</Child>

<style>
	[data-x="a©b"] {
		color: red;
	}
</style>
