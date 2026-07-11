<svelte:options css="injected" />

<script>
	import Child from './Child.svelte';
</script>

<Child>
	<div class=a&#38;&#x62;>x</div>
</Child>

<style>
	.a\&b {
		color: red;
	}

	.unused-prune {
		color: blue;
	}
</style>
