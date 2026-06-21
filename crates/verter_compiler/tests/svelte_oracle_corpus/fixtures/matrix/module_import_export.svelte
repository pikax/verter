<script module>
	import { base } from "./base.js";
	export const VERSION = 1;
</script>
<script>
	let count = $state(base);
</script>
<button onclick={() => count++}>{count}</button>
