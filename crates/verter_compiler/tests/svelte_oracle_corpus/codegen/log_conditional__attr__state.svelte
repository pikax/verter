<script>
	let n = $state(0);
</script>

<div onclick={() => n++} id={n && n ? 1 : 2}></div>
