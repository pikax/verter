<script>
	let a = $state(1);
	let b = $state(2);
</script>

{@debug a, b}
<button onclick={() => a++}>a</button>
<button onclick={() => b++}>b</button>
