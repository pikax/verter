<script>
	let n = $state(0);
</script>

<div onclick={() => n++} class:on={n ? 'a' : 'b'}></div>
