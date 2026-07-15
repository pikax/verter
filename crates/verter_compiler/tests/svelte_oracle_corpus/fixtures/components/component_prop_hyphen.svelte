<script>
	import Child from './Child.svelte';
	let { x } = $props();
</script>

<Child aria-label={x} />
