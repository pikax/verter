<svelte:options name="x" />
<script>let c = $state(0);</script>
<button onclick={() => c++}>{c}</button>
