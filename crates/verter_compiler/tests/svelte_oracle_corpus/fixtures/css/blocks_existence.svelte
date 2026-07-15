<script>
	let open = $state(true);
	let { rows } = $props();
</script>

{#if open}
	<p class="note">shown</p>
{/if}

<ul>
	{#each rows as row}
		<li class="row">{row}</li>
	{/each}
</ul>

<style>
	.note {
		color: red;
	}

	.row {
		color: blue;
	}

	.gone {
		color: green;
	}
</style>
