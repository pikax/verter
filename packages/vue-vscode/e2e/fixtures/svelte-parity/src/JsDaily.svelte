<script>
  // @ts-check
  /** @type {{ label: string, count: number }} */
  let jsDaily = { label: "js", count: 2 };
  /** @param {{ label: string }} value */
  function renderJsDaily(value) {
    return value.label.toUpperCase();
  }
  let jsDailyRendered = renderJsDaily(jsDaily);
</script>

<p>{jsDailyRendered} {jsDaily.label}</p>
