<script>
	let { rows } = $props();
	let n = $state(0);
</script>

{#each rows as row, i}
	<button onclick={() => n = i}>x</button>
{/each}
