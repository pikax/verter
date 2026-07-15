<script>
	import Child from './Child.svelte';
</script>

<Child>
	<div class="a a&b">x</div>
</Child>

<style>
	:global(.a\&b) {
		color: red;
	}
</style>
