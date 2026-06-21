<svelte:options customElement="a{x}b" />
<script>let c = $state(0);</script>
<button onclick={() => c++}>{c}</button>
