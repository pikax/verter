<script module>
	import './setup.js';
	import { m } from './m.js';
</script>

<script>
	let c = $state(0);
</script>

<p>{m}</p>
<button onclick={() => c++}>{c}</button>
