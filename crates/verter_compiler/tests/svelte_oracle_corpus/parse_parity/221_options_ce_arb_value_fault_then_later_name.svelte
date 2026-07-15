<svelte:options customElement={42} name="x" />
<script>let c = $state(0);</script>
<button onclick={() => c++}>{c}</button>
