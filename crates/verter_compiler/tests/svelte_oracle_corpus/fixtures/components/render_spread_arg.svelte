<script>
	let { xs } = $props();
</script>

{#snippet row(items)}<p>{items}</p>{/snippet}
{@render row([...xs])}
