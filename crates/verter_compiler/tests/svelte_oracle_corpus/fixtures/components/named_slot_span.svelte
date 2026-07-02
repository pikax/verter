<script>
	import Child from './Child.svelte';
	let { x } = $props();
</script>

<Child><span slot="foo-bar">{x}</span></Child>
