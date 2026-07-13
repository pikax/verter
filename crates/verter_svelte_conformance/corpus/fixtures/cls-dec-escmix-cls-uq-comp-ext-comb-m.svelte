<script>
	import Child from './Child.svelte';
</script>

<Child>
	<div class="wrap">
	<div class=a&#38;b&#38;c>x</div>
</div>
</Child>

<style>
	.wrap .a\26 b\&c {
		color: red;
	}
</style>
