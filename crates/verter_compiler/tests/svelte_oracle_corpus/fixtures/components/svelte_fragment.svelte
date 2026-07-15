<script>
	import Child from './Child.svelte';
	let { p } = $props();
</script>

<Child>
	<svelte:fragment slot="header">
		<span>h</span>
	</svelte:fragment>
</Child>
