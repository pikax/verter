<script>
	import { writable } from 'svelte/store';

	const c = writable(0);

	function set5() {
		$c = 5;
	}
</script>

<button onclick={set5}>{$c}</button>
<button onclick={() => { $c = 0; }}>reset</button>
