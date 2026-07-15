<script>
	import { one } from './one.js';
	import { two } from './two.js';
	import { three } from './three.js';

	let c = $state(0);
</script>

<p>{one} {two} {three}</p>
<button onclick={() => c++}>{c}</button>
