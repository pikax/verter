<script>
	import Child from './Child.svelte';
	let { flag } = $props();
</script>

<Child>
	<div id={flag ? 'a&b' : 'a\u0026b'}>x</div>
</Child>

<style>
	:global(#a\&b) {
		color: red;
	}
</style>
