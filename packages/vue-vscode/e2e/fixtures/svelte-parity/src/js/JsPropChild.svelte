<script>
  // @ts-check
  /**
   * @typedef {{ title: string, count: number }} JsPropChildProps
   */
  /** @type {JsPropChildProps} */
  let { title, count } = $props();
</script>

<span>{title}:{count}</span>
