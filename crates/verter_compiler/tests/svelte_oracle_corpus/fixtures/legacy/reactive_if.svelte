<script>
	let x = 0;
	let big = false;
	$: if (x > 5) { big = true; }
</script>

<p>{big}</p>
<button onclick={() => x++}>b</button>
