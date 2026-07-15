<script>
	import d, * as ns from './c.js';

	let c = $state(0);
</script>

<p>{d} {ns.k}</p>
<button onclick={() => c++}>{c}</button>
