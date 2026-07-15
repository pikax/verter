<script>
	import Child from './Child.svelte';
</script>

<Child>
	<div class="wrap">
	<div class=a&b>x</div>
</div>
</Child>

<style>
	.wrap [data-x="\zz"] {
		color: red;
	}
</style>
