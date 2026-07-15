<svelte:options customElement={42} />
</div>
<script>let c = $state(0);</script>
<button onclick={() => c++}>{c}</button>
