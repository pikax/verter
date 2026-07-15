<script>
	let msg = $state("a");
	let n = $state(0);
</script>
<button onclick={() => { msg = `v${n}`; }}>{msg}</button>
