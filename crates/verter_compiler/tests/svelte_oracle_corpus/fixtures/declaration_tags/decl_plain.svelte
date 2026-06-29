<script>
	let { items } = $props();
	let n = $state(0);
</script>

{#each items as item}
	{const doubled = item}
	<button onclick={() => n = doubled}>x</button>
{/each}
