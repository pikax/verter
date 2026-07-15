<script>
	let o = $state({ a: 1 });
	let snap = $state(null);
</script>

<button onclick={() => snap = $state.snapshot(o)}>{snap}</button>
