<svelte:options customElement={1,2} />
<script>let c = $state(0);</script>
<button onclick={() => c++}>{c}</button>
