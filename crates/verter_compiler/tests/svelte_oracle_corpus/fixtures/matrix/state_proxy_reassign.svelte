<script>
	let o = $state({ x: 0 });
</script>
<button onclick={() => o = { x: 1 }}>{o.x}</button>
