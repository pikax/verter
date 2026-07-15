<script>
	import Child from './Child.svelte';
	let { value } = $props();
</script>

<Child>
	<div id="{value}">x</div>
</Child>

<style>
	#b {
		color: red;

		&:hover {
			color: blue;
		}
	}
</style>
