<script>
	import Child from './Child.svelte';
	let { value } = $props();
</script>

<Child>
	<div id={value}>x</div>
</Child>

<style>
	#a\26 b\&c {
		color: red;
	}

	.unused-prune {
		color: blue;
	}
</style>
