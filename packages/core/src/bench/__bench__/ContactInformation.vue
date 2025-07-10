<script setup lang="ts">
const props = defineProps<{
  item: Contact;
}>();
const { i18n, $ts: ts } = useI18n();

// TODO handle expanded
// when expanded it will show all the extra phones and such
</script>

<template>
  <tabled-content>
    <tabled-content-row
      v-if="!item.isCompany && item.company"
      :label="`${i18n.contact.form.info.company.label} / ${i18n.contact.form.info.companyTitle.label}`"
      :value="item.company"
    >
      <div class="flex">
        <contact-display :value="item.company" /> /
        {{ item.companyTitle || "none" }}
      </div>
    </tabled-content-row>

    <tabled-content-row :value="item.phones.find((x) => x.primary)">
      <template #label>
        <span>{{ i18n.contact.form.info.phones.label }}</span>
        <h-pill v-if="item.phones.length > 1">
          {{ ts("contact.form.info.phones.more", [item.phones.length - 1]) }}
        </h-pill>
      </template>
      <template #default="{ value, type }">
        <div v-if="value">
          <a :href="`tel:${value}`"> {{ value }}</a> <span> ({{ type }})</span>
        </div>
        <span v-else>—</span>
      </template>
    </tabled-content-row>

    <tabled-content-row :value="item.emails.find((x) => x.primary)">
      <template #label>
        <span>{{ i18n.contact.form.info.emails.label }}</span>
        <h-pill v-if="item.emails.length > 1">
          {{ ts("contact.form.info.emails.more", [item.emails.length - 1]) }}
        </h-pill>
      </template>
      <template #default="{ value, type }">
        <a :href="`mailto:${value}`"> {{ value }}</a>
        <span> ({{ type }})</span>
      </template>
    </tabled-content-row>

    <tabled-content-row :value="item.websites.find((x) => x.primary)">
      <template #label>
        <span>{{ i18n.contact.form.info.websites.label }}</span>
        <h-pill v-if="item.websites.length > 1">
          {{
            ts("contact.form.info.websites.more", [item.websites.length - 1])
          }}
        </h-pill>
      </template>
      <template #default="{ value, type }">
        <a :href="value" target="_bank">
          <span>{{ value }}</span>
          <h-icon class="mx-1 inline-block" icon="ic:round-launch" />
        </a>
        <span> ({{ type }})</span>
      </template>
    </tabled-content-row>

    <tabled-content-row :value="item.addresses.find((x) => x.primary)">
      <template #label>
        <span>{{ i18n.contact.form.info.addresses.label }}</span>
        <h-pill v-if="item.addresses.length > 1">
          {{
            ts("contact.form.info.addresses.more", [item.addresses.length - 1])
          }}
        </h-pill>
      </template>
      <template #default="address">
        <div class="relative pr-8">
          <p class="font-semibold text-gray-500">{{ address.type }}</p>
          <p class="whitespace-pre-wrap">{{ address.street }}</p>
          <p>{{ address.postCode }}</p>
          <p>{{ address.country }}</p>

          <div class="absolute right-0 top-0">
            <button>
              <h-icon icon="ic:round-copy-all" />
              <!-- TODO copy to clipboard -->
            </button>
            <a
              target="_blank"
              :href="`http://maps.google.com/?q=${[
                address.street,
                address.postCode,
                address.country,
              ]
                .filter(Boolean)
                .join(', ')}`"
            >
              <!-- TODO open google maps -->
              <h-icon icon="ic:round-map" />
            </a>
          </div>
        </div>

        <!-- <a :href="value" target="_bank">
            <span>{{ value }}</span>
            <h-icon icon="ic:round-launch" />
          </a>
          ({{ type }}) -->
      </template>
    </tabled-content-row>
    <!-- TODO others -->
  </tabled-content>
</template>
