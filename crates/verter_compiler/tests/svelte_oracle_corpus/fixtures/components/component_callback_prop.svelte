<script>
	import Child from './Child.svelte';
	let { p } = $props();
</script>

<Child onfoo={() => p} />
