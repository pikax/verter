<script>
	import Child from './Child.svelte';
</script>

<Child>
	<div class="wrap">
	<div class="a a&b">x</div>
</div>
</Child>

<style>
	.wrap #\zz {
		color: red;
	}
</style>
