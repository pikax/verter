<svelte:options customElement={foo bar} />
<script>let c = $state(0);</script>
<button onclick={() => c++}>{c}</button>
