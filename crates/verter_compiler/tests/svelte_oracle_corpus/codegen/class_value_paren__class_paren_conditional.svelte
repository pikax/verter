<script>
	let a = $state(0);
</script>

<div class={(a ? 'x' : 'y')} onclick={() => a++}></div>
