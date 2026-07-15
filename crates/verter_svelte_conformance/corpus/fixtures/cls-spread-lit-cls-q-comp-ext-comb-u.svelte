<script>
	import Child from './Child.svelte';
	let { rest } = $props();
</script>

<Child>
	<div class="wrap">
	<div {...rest}>x</div>
</div>
</Child>

<style>
	.wrap .b {
		color: red;
	}
</style>
