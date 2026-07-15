<script>
	import Child from './Child.svelte';
	import Inner from './Inner.svelte';
	let { x } = $props();
</script>

<Child>
	<svelte:fragment slot="head">
		<Inner slot="x" label={x} />
	</svelte:fragment>
</Child>
