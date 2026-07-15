<script>
	import Child from './Child.svelte';
	let { rest } = $props();
</script>

<Child>
	<div {...rest}>x</div>
</Child>

<style>
	p {
		color: red;
	}
</style>
