<script>
	let { promise } = $props();
</script>

{#await promise then value}
	<p>{value}</p>
{/await}
