<script>
	import d from './a.js';
	import { n } from './b.js';
	import * as NS from './c.js';

	let c = $state(0);
</script>

<p>{d} {n} {NS}</p>
<button onclick={() => c++}>{c}</button>
