<svelte:options css="injected" />

<script>
	import Child from './Child.svelte';
</script>

<Child>
	<div id=a&#x26;b&#x26;c>x</div>
</Child>

<style>
	[data-x="\7a \z"] {
		color: red;
	}

	.unused-prune {
		color: blue;
	}
</style>
