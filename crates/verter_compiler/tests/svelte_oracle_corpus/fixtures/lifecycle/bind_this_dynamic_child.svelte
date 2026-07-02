<script>
	let c = $state(0);
	let el = $state(null);
</script>

<div bind:this={el} onclick={() => c++}><span>{c}</span></div>
