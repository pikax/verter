<svelte:options customElement={{ tag: 1 }} />
<script>let c = $state(0);</script>
<button onclick={() => c++}>{c}</button>
