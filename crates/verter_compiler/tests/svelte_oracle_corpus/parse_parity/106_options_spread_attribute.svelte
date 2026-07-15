<svelte:options {...opts} />
<script>let c = $state(0);</script>
<button onclick={() => c++}>{c}</button>
