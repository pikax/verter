<script>
	let width = $state(0);
	let scroll = $state(0);
</script>

<svelte:window bind:innerWidth={width} onresize={() => scroll++} bind:scrollX={scroll} onkeydown={() => width++} />
