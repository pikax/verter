<script>
	let x = 0;
	$: z = y + 1;
	$: y = x + 1;
</script>

<p>{z}</p>
<button onclick={() => x++}>b</button>
