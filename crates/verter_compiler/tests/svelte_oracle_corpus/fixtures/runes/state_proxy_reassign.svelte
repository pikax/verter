<script>
	let o = $state({ a: 1 });
</script>

<button onclick={() => o = { a: 2 }}>{o}</button>
