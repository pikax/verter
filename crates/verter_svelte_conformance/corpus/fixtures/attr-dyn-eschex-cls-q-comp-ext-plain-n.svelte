<script>
	import Child from './Child.svelte';
	let { flag } = $props();
</script>

<Child>
	<div class="{flag ? 'a a&b' : 'a\u0026b'}">x</div>
</Child>

<style>
	[data-x="\7a z"] {
		color: red;
	}
</style>
