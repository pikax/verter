<script>
	let x = 0;
	$: (y = x + 1);
</script>

<p>{y}</p>
<button onclick={() => x++}>b</button>
