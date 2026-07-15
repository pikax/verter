<script>
	import Child from './Child.svelte';
	let { x } = $props();
</script>

<Child><svelte:element this="div" slot="foo" id={x} /></Child>
