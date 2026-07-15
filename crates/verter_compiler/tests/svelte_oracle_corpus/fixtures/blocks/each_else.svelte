<script>
	let { rows } = $props();
</script>

{#each rows as row}
	<p>{row}</p>
{:else}
	<p>empty</p>
{/each}
