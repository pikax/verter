<script>
	let c2 = $state(0);
</script>
<button onclick={() => { $effect.root(() => { return () => {}; }); c2++; }}>{c2}</button>
