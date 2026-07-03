<script>
	let o = $state.raw({ a: 1 });
</script>

<button onclick={() => o = { a: 2 }}>{o}</button>
