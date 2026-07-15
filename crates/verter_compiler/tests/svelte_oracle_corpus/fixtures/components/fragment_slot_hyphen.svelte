<script>
	import Child from './Child.svelte';
	let { x } = $props();
</script>

<Child>
	<svelte:fragment slot="foo-bar"><span>{x}</span></svelte:fragment>
</Child>
