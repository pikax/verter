<svelte:options customElement={{ tag: "my-el" }} />
<script>let c = $state(0);</script>
<button onclick={() => c++}>{c}</button>
