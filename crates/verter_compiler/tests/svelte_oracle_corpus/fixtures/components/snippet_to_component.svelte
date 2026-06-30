<script>
	import Child from './Child.svelte';
	let { p } = $props();
</script>

{#snippet tmpl(item)}<span>{item} {p}</span>{/snippet}

<Child {tmpl} />
