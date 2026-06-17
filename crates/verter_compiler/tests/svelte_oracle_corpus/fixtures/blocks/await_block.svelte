<script>
	let promise = $state(Promise.resolve(42));
</script>

{#await promise}
	<p>loading…</p>
{:then value}
	<p>got {value}</p>
{:catch error}
	<p>error {error.message}</p>
{/await}
