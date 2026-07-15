<script>
	let { promise } = $props();
</script>

{#await promise}
	<p>loading</p>
{:catch error}
	<p>failed</p>
{/await}
