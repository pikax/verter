<svelte:options customElement={{ tag: }} />
<script>let c = $state(0);</script>
<button onclick={() => c++}>{c}</button>
