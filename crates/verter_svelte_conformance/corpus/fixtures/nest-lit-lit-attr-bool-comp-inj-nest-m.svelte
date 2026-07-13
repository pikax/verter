<svelte:options css="injected" />

<script>
	import Child from './Child.svelte';
</script>

<Child>
	<div data-x>x</div>
</Child>

<style>
	[data-x] {
		& {
			color: red;
		}
	}
</style>
