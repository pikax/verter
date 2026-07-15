<script>
	let { rows } = $props();
</script>

{#each rows as row}
	<button onmouseenter={() => row.x++}>x</button>
{/each}
