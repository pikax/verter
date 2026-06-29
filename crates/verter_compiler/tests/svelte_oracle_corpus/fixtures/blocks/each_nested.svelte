<script>
	let { grid } = $props();
</script>

{#each grid as cells}
	{#each cells as cell}
		<p>{cell}</p>
	{/each}
{/each}
