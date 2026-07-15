<script>
	import Child from './Child.svelte';
	let { x } = $props();
</script>

<Child><svelte:fragment slot="foo">hello {x}</svelte:fragment></Child>
