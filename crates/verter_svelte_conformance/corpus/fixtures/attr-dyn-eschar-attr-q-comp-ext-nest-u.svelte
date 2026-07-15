<script>
	import Child from './Child.svelte';
	let { value } = $props();
</script>

<Child>
	<div data-x="{value}">x</div>
</Child>

<style>
	[data-x="a\&b"] {
		color: red;

		&:hover {
			color: blue;
		}
	}
</style>
