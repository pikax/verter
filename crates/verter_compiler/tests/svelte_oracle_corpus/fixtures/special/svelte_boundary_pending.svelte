<script>
	let { x } = $props();
</script>

<svelte:boundary>
	<p>{x}</p>
	{#snippet pending()}
		<p>loading</p>
	{/snippet}
</svelte:boundary>
