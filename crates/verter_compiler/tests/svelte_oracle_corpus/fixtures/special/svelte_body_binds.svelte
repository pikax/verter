<script>
	let n = $state(0);
	let cw = $state(0);
	let ch = $state(0);
</script>

<svelte:body onclick={() => n++} bind:clientWidth={cw} bind:clientHeight={ch} />
