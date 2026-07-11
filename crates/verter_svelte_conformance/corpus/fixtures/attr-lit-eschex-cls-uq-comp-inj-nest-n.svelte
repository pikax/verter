<svelte:options css="injected" />

<script>
	import Child from './Child.svelte';
</script>

<Child>
	<div class=a&b>x</div>
</Child>

<style>
	[data-x="\7a z"] {
		color: red;

		&:hover {
			color: blue;
		}
	}
</style>
