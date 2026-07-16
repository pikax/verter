<script lang="ts" generic="T = string">
  /** Defaulted generic — no type args required at call site. */
  let {
    value,
    prefix = "",
  }: {
    value?: T;
    prefix?: string;
  } = $props();
</script>

<span>{prefix}{value}</span>
