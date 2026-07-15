<script>
	let { promise } = $props();
</script>

{#await promise}{:catch error}
	<p>failed</p>
{/await}
