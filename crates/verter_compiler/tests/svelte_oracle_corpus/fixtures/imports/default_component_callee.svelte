<script>
	import Child from './Child.svelte';

	let c = $state(0);
</script>

<Child />
<button onclick={() => c++}>{c}</button>
