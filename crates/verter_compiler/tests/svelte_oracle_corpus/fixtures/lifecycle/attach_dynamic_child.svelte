<script>
	let c = $state(0);
</script>

<div {@attach foo} onclick={() => c++}><span>{c}</span></div>
