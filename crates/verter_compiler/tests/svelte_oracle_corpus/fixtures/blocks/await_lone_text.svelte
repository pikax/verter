<script>
	let { promise } = $props();
</script>

{#await promise}loading{:then value}{value}{:catch e}failed{/await}
