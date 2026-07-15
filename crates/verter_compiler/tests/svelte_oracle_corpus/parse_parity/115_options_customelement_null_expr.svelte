<svelte:options customElement={null} />
<script>let c = $state(0);</script>
<button onclick={() => c++}>{c}</button>
