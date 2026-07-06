<script module>
	import { m } from './base.js';
</script>

<script>
	import { i } from './inst.js';

	let c = $state(0);
</script>

<p>{m} {i}</p>
<button onclick={() => c++}>{c}</button>
