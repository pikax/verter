<script>
	let { depth } = $props();
</script>

{#if depth > 0}
	<svelte:self slot="a" depth={depth - 1} />
{/if}
