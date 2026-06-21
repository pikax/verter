<script>let c = $state(0);</script>
<div bind:this={$foo}></div>
<button onclick={() => c++}>{c}</button>
