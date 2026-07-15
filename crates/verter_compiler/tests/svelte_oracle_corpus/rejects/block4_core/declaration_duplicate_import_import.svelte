<script>
	import { x } from './a.js';
	import { x } from './b.js';

	let c = $state(0);
</script>

<button onclick={() => c++}>{c} {x}</button>
