<script>
	let { depth } = $props();
</script>

{#if depth > 0}
	<svelte:self depth={depth - 1} />
{/if}
