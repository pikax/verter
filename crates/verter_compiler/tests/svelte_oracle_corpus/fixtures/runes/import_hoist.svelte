<script>
	import DEFAULT from './constants.js';
	let count = $state(0);
</script>
<button onclick={() => { count = DEFAULT; }}>{count}</button>
