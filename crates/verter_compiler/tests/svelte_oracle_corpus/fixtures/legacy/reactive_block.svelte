<script>
	let x = 0;
	let t = 0;
	$: { t = x * 2; console.log(t); }
</script>

<p>{t}</p>
<button onclick={() => x++}>b</button>
