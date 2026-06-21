<svelte:options customElement={1 2} foo foo />
<script>let c = $state(0);</script>
<button onclick={() => c++}>{c}</button>
