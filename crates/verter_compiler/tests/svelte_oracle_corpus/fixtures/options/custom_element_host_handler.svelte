<svelte:options customElement="x-host" />

<script>
	let c = $state(0);
</script>

<button onfocus={() => $host().dispatchEvent(new CustomEvent('boop'))}>hi</button>
<button onclick={() => c++}>{c}</button>
