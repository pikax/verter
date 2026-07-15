<script>
	import Child from './Child.svelte';
	import Inner from './Inner.svelte';
	let { x } = $props();
</script>

<Child><Inner slot="foo" label={x} /></Child>
