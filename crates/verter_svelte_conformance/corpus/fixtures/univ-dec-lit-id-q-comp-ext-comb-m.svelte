<script>
	import Child from './Child.svelte';
</script>

<Child>
	<div class="wrap">
	<div id="&#98;">x</div>
</div>
</Child>

<style>
	.wrap * {
		color: red;
	}
</style>
