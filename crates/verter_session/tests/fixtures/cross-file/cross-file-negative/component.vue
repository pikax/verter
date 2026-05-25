<script setup lang="ts">
import type { CarrierProps } from './types';
defineProps<CarrierProps>();
defineEmits<{ submit: [event: SubmitEvent] }>();
</script>
