<script>
	import { x, y } from './m.js';

	let c = $state(0);
</script>

<p>{x} {y}</p>
<button onclick={() => c++}>{c}</button>
