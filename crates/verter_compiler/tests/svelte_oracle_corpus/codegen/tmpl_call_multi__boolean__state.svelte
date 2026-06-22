<script>
	let n = $state(0);
</script>

<input onclick={() => n++} disabled="a {n.toString()} b {n.toString()} c" />
