<script lang="ts">
  interface DailyValue {
    label: string;
    count: number;
  }

  let dailyValue: DailyValue = { label: "daily", count: 3 };

  function renderDaily(): string {
    return `${dailyValue.label}:${dailyValue.count}`;
  }
</script>

<section>
  <button onclick={renderDaily}>{dailyValue.label}</button>
  <span>{dailyValue.count}</span>
</section>
