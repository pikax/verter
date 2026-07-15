<script>
	import { writable } from 'svelte/store';

	const c = writable(0);

	function bump() {
		$c++;
	}

	function down() {
		$c--;
	}

	function pre() {
		++$c;
	}

	function add() {
		$c += 2;
	}
</script>

<button onclick={bump}>{$c}</button>
<button onclick={down}>down</button>
<button onclick={pre}>pre</button>
<button onclick={add}>add</button>
