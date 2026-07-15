<svelte:options css="external" />
<script>let c = $state(0);</script>
<button onclick={() => c++}>{c}</button>
