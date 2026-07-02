<script>
	import Child from './Child.svelte';
	let { x } = $props();
</script>

<Child><span slot="foo&amp;bar">{x}</span></Child>
