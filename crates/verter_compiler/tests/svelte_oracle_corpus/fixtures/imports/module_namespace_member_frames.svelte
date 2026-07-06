<script module>
	import * as NS from './m.js';
</script>

<script>
	let c = $state(0);
</script>

<p>{NS.z}</p>
<button onclick={() => c++}>{c}</button>
