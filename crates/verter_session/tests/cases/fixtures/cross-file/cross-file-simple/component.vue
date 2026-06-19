<script setup lang="ts">
import type { FooProps } from './types';
defineProps<FooProps>();
defineEmits<{ submit: [event: SubmitEvent] }>();
</script>
