<script>
	import Child from './Child.svelte';
</script>

<Child>
	<div class="wrap">
	<div data-x=a&copy;b>x</div>
</div>
</Child>

<style>
	.wrap .zz {
		color: red;
	}
</style>
