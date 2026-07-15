<script>
	import Child from './Child.svelte';
	let { p } = $props();
</script>

<Child let:item={value}>{value}</Child>
