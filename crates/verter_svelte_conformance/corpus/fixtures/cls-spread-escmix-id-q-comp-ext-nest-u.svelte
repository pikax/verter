<script>
	import Child from './Child.svelte';
	let { rest } = $props();
</script>

<Child>
	<div {...rest}>x</div>
</Child>

<style>
	.a\26 b\&c {
		color: red;

		&:hover {
			color: blue;
		}
	}
</style>
