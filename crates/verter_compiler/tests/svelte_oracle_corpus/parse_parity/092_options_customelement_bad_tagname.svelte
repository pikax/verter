<svelte:options customElement="nodash" />
<script>let c = $state(0);</script>
<button onclick={() => c++}>{c}</button>
