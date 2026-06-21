<svelte:options customElement={{ props: 1 }} />
<script>let c = $state(0);</script>
<button onclick={() => c++}>{c}</button>
