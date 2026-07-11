<script>
	import Child from './Child.svelte';
</script>

<Child>
	<div class="wrap">
	<div id="a&#38;&#x62;">x</div>
</div>
</Child>

<style>
	.wrap #a\26 b {
		color: red;
	}
</style>
