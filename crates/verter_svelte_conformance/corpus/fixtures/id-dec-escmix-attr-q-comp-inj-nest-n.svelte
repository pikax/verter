<svelte:options css="injected" />

<script>
	import Child from './Child.svelte';
</script>

<Child>
	<div data-x="a&#38;b&#38;c">x</div>
</Child>

<style>
	#\7a \z {
		color: red;

		&:hover {
			color: blue;
		}
	}
</style>
