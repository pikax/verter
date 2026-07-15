<svelte:options customElement={{ tag: 'x-shadow', shadow: { mode: 'open', delegatesFocus: true } }} />

<script>
	let c = $state(0);
</script>

<button onclick={() => c++}>{c}</button>
