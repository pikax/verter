<script>
	let n = $state(0);
</script>

<div onclick={() => n++} id={n > 0 ? -n : 0}></div>
