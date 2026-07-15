<script>
	let { failedProp } = $props();
</script>

<svelte:boundary failed={failedProp}>
	<p>content</p>
	{#snippet failed(error, reset)}
		<p>oops</p>
	{/snippet}
</svelte:boundary>
