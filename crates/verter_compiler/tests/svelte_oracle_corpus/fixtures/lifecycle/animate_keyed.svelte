<script>
	let { items } = $props();
</script>

{#each items as item (item.id)}
	<div animate:flip>{item}</div>
{/each}
