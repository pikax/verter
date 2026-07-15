<script>let c = $state(0); $effect(() => { c++; $inspect.trace(); });</script>
<p>{c}</p>
