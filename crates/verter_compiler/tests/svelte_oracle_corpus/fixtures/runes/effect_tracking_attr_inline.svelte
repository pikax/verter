<script>
	let c = $state(0);
</script>

<input disabled={$effect.tracking()} />
<button onclick={() => c++}>{c}</button>
