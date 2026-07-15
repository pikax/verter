<script>let c = $state(0);</script>
<input bind:value={c} value={c} />
<button onclick={() => c++}>{c}</button>
