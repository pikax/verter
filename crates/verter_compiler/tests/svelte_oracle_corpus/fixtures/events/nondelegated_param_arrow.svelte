<script>
	let n = $state(0);
</script>
<input onfocus={(e) => n++} />
<p>{n}</p>
