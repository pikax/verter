<script>
	import Child from './Child.svelte';
</script>

<Child>
	<div data-x>x</div>
</Child>

<style>
	:global([data-x]) {
		color: red;
	}
</style>
