<script>
	let muted = $state(false);
</script>

<video onclick={() => muted = !muted} muted={muted}></video>
