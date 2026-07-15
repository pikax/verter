<svelte:options customElement={{ shadow: "open" }} />
<script>let c = $state(0);</script>
<button onclick={() => c++}>{c}</button>
