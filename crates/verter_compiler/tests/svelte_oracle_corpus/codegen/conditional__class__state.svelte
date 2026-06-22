<script>
	let n = $state(0);
</script>

<div onclick={() => n++} class={n ? 'a' : 'b'}></div>
