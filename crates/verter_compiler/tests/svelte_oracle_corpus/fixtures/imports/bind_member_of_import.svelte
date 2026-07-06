<script>
	import { x } from './m.js';

	let c = $state(0);
</script>

<input bind:value={x.k} />
<button onclick={() => c++}>{c}</button>
