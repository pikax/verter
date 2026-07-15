<script>
	let i = 0;
	$: for (const i of [1, 2]) { console.log(i); }
</script>

<p>{i}</p>
<button onclick={() => i++}>b</button>
