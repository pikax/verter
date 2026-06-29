<script>
	let { items } = $props();
</script>

{#each items as item}
	{let local = $state(0)}
	<button onclick={() => local++}>{local}</button>
{/each}
