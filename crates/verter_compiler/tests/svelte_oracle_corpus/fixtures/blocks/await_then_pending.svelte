<script>
	let { promise } = $props();
</script>

{#await promise}
	<p>loading</p>
{:then value}
	<p>{value}</p>
{/await}
