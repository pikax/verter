<script>
	let count = $state(0);
</script>

<button onclick={() => count++}>click {count}</button>
<div onmouseenter={() => {}} onmouseleave={() => {}}>hover</div>
<input oninput={(e) => count = e.currentTarget.value.length} />
