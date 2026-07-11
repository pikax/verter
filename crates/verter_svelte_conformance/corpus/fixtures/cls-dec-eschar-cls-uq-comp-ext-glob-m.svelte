<script>
	import Child from './Child.svelte';
</script>

<Child>
	<div class=a&#38;b>x</div>
</Child>

<style>
	:global(.a\&b) {
		color: red;
	}
</style>
