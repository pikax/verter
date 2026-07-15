<script>
	let form = $state({ name: "" });
</script>
<input bind:value={form.name} />
<p>{form.name}</p>
