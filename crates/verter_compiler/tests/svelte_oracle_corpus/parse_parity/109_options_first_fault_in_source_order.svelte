<svelte:options name="x" namespace="bad" />
<script>let c = $state(0);</script>
<button onclick={() => c++}>{c}</button>
