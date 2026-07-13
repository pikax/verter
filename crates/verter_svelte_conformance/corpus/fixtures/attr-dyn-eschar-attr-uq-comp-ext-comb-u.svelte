<script>
	import Child from './Child.svelte';
	let { value } = $props();
</script>

<Child>
	<div class="wrap">
	<div data-x={value}>x</div>
</div>
</Child>

<style>
	.wrap [data-x="a\&b"] {
		color: red;
	}
</style>
