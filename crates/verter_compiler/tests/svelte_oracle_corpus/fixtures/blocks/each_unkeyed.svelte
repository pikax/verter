<script>
	let { rows } = $props();
</script>

{#each rows as row}
	<p>{row}</p>
{/each}
