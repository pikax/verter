<svelte:options foo foo customElement={} />
<script>let c = $state(0);</script>
<button onclick={() => c++}>{c}</button>
