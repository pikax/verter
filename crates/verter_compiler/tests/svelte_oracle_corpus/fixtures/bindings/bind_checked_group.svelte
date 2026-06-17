<script>
	let agreed = $state(false);
	let picks = $state([]);
</script>

<input type="checkbox" bind:checked={agreed} />
<input type="checkbox" bind:group={picks} value="a" />
<input type="checkbox" bind:group={picks} value="b" />
