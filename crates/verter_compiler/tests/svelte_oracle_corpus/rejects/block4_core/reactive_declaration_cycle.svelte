<script>
	let x = 0;
	$: a = b + x;
	$: b = a + 1;
</script>

<p>{a}</p>
<button onclick={() => x++}>b</button>
