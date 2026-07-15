<script>
	import Child from './Child.svelte';
	let { value } = $props();
</script>

<Child>
	<div class="wrap">
	<div id={value}>x</div>
</div>
</Child>

<style>
	.wrap #b {
		color: red;
	}
</style>
