<script>
	import Child from './Child.svelte';
</script>

<Child>
	<div data-x="a&copy;b">x</div>
</Child>

<style>
	:global([data-x="a©b"]) {
		color: red;
	}
</style>
