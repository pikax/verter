<script>
	let c = $state(0);
</script>
<button onclick={() => { $effect(() => console.log(c)); c++; }}>{c}</button>
