<script>
	let { items } = $props();
</script>

{#each items as item (item.id)}
	{@const l = item.n}
	<div animate:flip>{l}</div>
{/each}
