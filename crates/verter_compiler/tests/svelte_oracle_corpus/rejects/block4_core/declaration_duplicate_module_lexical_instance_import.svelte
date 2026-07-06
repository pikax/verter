<script module>
	let k = 1;
</script>

<script>
	import { k } from './m.js';

	let c = $state(0);
</script>

<button onclick={() => c++}>{c}</button>
