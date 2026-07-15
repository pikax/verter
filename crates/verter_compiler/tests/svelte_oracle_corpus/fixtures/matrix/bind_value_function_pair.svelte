<script>
	let value = $state("");
</script>
<input bind:value={() => value, (next) => value = next} />
