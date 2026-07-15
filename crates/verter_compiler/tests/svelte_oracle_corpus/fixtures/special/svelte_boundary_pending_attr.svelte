<script>
	let { pending } = $props();
</script>

<svelte:boundary pending={pending}>
	<p>content</p>
</svelte:boundary>
