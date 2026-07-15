<script>
	import { x } from './m.js';

	let c = $state(0);
</script>

<button onmouseenter={() => x = 1} onclick={() => c++}>{c}</button>
