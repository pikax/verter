<script module>
	import { x } from './m.js';
</script>

<script>
	let x = $state(0);
</script>

<button onclick={() => x++}>{x}</button>
