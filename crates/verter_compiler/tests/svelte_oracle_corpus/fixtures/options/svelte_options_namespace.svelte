<svelte:options namespace="svg" />

<script>
	let r = $state(10);
</script>

<circle cx="50" cy="50" r={r} />
