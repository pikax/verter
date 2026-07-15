<svelte:options customElement={42} foo foo />
<script>let c = $state(0);</script>
<button onclick={() => c++}>{c}</button>
