<script>
	import Child from './Child.svelte';
</script>

<Child>
	<div id=a&amp;b&amp;c>x</div>
</Child>

<style>
	:global(#a\26 b\&c) {
		color: red;
	}
</style>
