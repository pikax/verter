<script>
	import { a } from './m.js';
	import { b } from './m.js';

	let c = $state(0);
</script>

<p>{a} {b}</p>
<button onclick={() => c++}>{c}</button>
