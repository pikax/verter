<script>
	import Child from './Child.svelte';
	let { depth } = $props();
</script>

<Child><svelte:self slot="foo" depth={depth - 1} /></Child>
