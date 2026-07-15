<script>
	let { failed } = $props();
</script>

<svelte:boundary failed={failed}>
	<p>content</p>
	{#snippet pending()}
		<p>loading</p>
	{/snippet}
</svelte:boundary>
