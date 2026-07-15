<script>
	import Child from './Child.svelte';
	let { p } = $props();
</script>

<Child>
	{#snippet header(item)}<span>{item}</span>{/snippet}
	default {p}
</Child>
