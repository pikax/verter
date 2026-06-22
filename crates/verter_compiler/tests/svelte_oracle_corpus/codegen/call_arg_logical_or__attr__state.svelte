<script>
	let n = $state(0);
</script>

<div onclick={() => n++} id={String(n || 1)}></div>
