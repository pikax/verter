<script>
	let form = { name: "" };
	let count = $state(0);
</script>
<input bind:value={form.name} />
<button onclick={() => count++}>{count}</button>
