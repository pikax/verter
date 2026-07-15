<script>
	let { items } = $props();
</script>

{#each items as item (item.id)}
	<div animate:flip={{ duration: 200 }}>{item}</div>
{/each}
