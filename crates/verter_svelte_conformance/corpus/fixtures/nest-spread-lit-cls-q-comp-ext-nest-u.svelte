<script>
	import Child from './Child.svelte';
	let { rest } = $props();
</script>

<Child>
	<div {...rest}>x</div>
</Child>

<style>
	.b {
		& {
			color: red;
		}
	}
</style>
