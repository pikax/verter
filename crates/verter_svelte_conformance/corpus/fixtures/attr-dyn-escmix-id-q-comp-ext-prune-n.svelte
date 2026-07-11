<script>
	import Child from './Child.svelte';
	let { flag } = $props();
</script>

<Child>
	<div id="{flag ? 'a&b&c' : 'a\u0026b\u0026c'}">x</div>
</Child>

<style>
	[data-x="\7a \z"] {
		color: red;
	}

	.unused-prune {
		color: blue;
	}
</style>
