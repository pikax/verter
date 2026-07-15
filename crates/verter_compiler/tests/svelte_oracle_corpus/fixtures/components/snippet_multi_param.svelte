<script>
	let { p } = $props();
</script>

{#snippet pair(a, b)}<span>{a} {b}</span>{/snippet}
{@render pair(1, 2)}
{p}
