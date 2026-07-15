<script>
	import Child from './Child.svelte';
	let { p } = $props();
	let child;
</script>

<Child bind:this={child} />
