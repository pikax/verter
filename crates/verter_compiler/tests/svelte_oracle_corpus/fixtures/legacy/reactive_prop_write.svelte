<script>
	export let p;
	let x = 0;
	$: p = x;
</script>

<p>{p}</p>
<button onclick={() => x++}>b</button>
