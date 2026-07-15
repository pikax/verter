<svelte:options customElement={1 + } />
<script>let c = $state(0);</script>
<button onclick={() => c++}>{c}</button>
