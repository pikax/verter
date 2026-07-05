<svelte:options customElement={{ tag: 'x-none', shadow: 'none' }} />

<script>
	let c = $state(0);
</script>

<button onclick={() => c++}>{c}</button>
