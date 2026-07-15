<svelte:options runes={true}>hi</svelte:options><script>let c = $state(0);</script>
<button onclick={() => c++}>{c}</button>
