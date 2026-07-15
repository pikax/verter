<script>
	let n = $state(0);
</script>

<div onclick={() => n++} id={n ? 'a' : 'b'}></div>
