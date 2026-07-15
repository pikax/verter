<script>
	let draft = "start";
	let count = $state(0);
</script>
<input bind:value={draft} />
<button onclick={() => count++}>{count}</button>
