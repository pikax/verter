<script>
	import Child from './Child.svelte';
	let { comp } = $props();
</script>

<Child><svelte:component this={comp} slot="foo" /></Child>
