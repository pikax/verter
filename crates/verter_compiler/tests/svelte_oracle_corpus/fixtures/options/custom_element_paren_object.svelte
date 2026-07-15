<svelte:options customElement={({ tag: 'x-paren' })} />

<script>
	let c = $state(0);
</script>

<button onclick={() => c++}>{c}</button>
