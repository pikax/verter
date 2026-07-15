<script>
	import Child from './Child.svelte';
	let { label } = $props();
</script>

<svelte:component this={Child} {label} />
