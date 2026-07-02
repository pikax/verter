<script>
	import Child from './Child.svelte';
	import Inner from './Inner.svelte';
	let { x } = $props();
</script>

<Child><div><Inner slot="bar" label={x} /></div></Child>
