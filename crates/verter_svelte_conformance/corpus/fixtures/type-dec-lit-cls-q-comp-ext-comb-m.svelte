<script>
	import Child from './Child.svelte';
</script>

<Child>
	<div class="wrap">
	<div class="a&#32;b">x</div>
</div>
</Child>

<style>
	.wrap div {
		color: red;
	}
</style>
