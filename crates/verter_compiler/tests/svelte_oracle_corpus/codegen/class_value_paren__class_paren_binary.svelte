<script>
	let a = $state(0);
	let b = $state(0);
</script>

<div class={((a + b))} onclick={() => { a++; b++; }}></div>
