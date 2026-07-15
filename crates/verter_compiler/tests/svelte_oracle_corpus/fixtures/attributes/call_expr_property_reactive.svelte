<script>
	let v = $state(false);
</script>

<input readonly={Boolean(v)} />
<button onclick={() => v = !v}>go</button>
