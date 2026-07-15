<script>
	import Child from './Child.svelte';
	let { flag } = $props();
</script>

<Child>
	<div id={flag ? 'b' : '\u0062'}>x</div>
</Child>

<style>
	.zz {
		color: red;
	}
</style>
