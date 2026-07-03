<script>
	let arr = $state(['']);
	let i = $state(0);
</script>

<input bind:value={arr[i]} />
<button onclick={() => i++}>next</button>
