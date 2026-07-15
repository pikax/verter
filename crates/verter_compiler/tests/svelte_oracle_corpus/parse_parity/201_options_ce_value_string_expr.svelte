<svelte:options customElement={"my-el"} />
<script>let c = $state(0);</script>
<button onclick={() => c++}>{c}</button>
