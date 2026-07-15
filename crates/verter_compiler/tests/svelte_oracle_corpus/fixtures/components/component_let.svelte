<script>
	import Child from './Child.svelte';
	let { p } = $props();
</script>

<Child let:item>{item}</Child>
