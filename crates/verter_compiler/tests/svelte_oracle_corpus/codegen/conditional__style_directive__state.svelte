<script>
	let n = $state(0);
</script>

<div onclick={() => n++} style:color={n ? 'a' : 'b'}></div>
