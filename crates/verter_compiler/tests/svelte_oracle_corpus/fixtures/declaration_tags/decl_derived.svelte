<script>
	let { items } = $props();
</script>

{#each items as item}
	{let doubled = $derived(item)}
	<p>{doubled}</p>
{/each}
