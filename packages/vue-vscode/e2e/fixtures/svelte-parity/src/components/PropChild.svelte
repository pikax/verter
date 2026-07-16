<script lang="ts">
  let {
    contractProp,
    optionalFlag = false,
  }: {
    contractProp: string;
    optionalFlag?: boolean;
  } = $props();
</script>

<span>{contractProp}:{optionalFlag}</span>
