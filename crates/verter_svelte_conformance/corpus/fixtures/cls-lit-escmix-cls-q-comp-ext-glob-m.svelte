<script>
	import Child from './Child.svelte';
</script>

<Child>
	<div class="a a&b&c">x</div>
</Child>

<style>
	:global(.a\26 b\&c) {
		color: red;
	}
</style>
