<script>
	import Child from './Child.svelte';
	let { value } = $props();
</script>

<Child>
	<div class="{value}">x</div>
</Child>

<style>
	.b {
		color: red;
	}
</style>
