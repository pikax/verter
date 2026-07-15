<script>
	import data from './d.json' with { type: 'json' };

	let c = $state(0);
</script>

<p>{data}</p>
<button onclick={() => c++}>{c}</button>
