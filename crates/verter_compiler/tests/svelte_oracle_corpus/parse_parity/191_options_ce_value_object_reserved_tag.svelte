<svelte:options customElement={{ tag: "annotation-xml" }} />
<script>let c = $state(0);</script>
<button onclick={() => c++}>{c}</button>
