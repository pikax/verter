<svelte:options customElement={{ tag: 'x-ext', shadow: 'none', extend: (c) => c }} />

<script>
	let c = $state(0);
</script>

<button onclick={() => c++}>{c}</button>
