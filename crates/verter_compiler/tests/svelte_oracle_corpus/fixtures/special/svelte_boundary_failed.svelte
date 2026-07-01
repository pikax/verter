<script>
	let { x } = $props();
</script>

<svelte:boundary>
	<p>{x}</p>
	{#snippet failed(error, reset)}
		<p>oops</p>
	{/snippet}
</svelte:boundary>
