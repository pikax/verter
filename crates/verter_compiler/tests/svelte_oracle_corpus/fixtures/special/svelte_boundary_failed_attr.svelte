<script>
	let { failed } = $props();
</script>

<svelte:boundary failed={failed}>
	<p>content</p>
</svelte:boundary>
