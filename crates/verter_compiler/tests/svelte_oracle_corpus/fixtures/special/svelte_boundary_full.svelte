<script>
	let n = $state(0);
</script>

<svelte:boundary onerror={() => n++}>
	<p>content</p>
	{#snippet failed(error, reset)}
		<p>oops</p>
	{/snippet}
	{#snippet pending()}
		<p>loading</p>
	{/snippet}
</svelte:boundary>
