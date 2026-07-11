<script>
	import Child from './Child.svelte';
	let { value } = $props();
</script>

<Child>
	<div class={value}>x</div>
</Child>

<style>
	.a\26 b {
		color: red;
	}
</style>
