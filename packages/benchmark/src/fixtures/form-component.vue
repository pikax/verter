<script setup lang="ts">
import { ref, computed } from 'vue'

interface FormData {
  firstName: string
  lastName: string
  email: string
  phone: string
  address: string
  city: string
  state: string
  zip: string
  country: string
  newsletter: boolean
  terms: boolean
}

const form = ref<FormData>({
  firstName: '',
  lastName: '',
  email: '',
  phone: '',
  address: '',
  city: '',
  state: '',
  zip: '',
  country: 'US',
  newsletter: false,
  terms: false
})

const errors = ref<Partial<Record<keyof FormData, string>>>({})

const isValid = computed(() => {
  return form.value.firstName.trim().length > 0 &&
         form.value.lastName.trim().length > 0 &&
         form.value.email.includes('@') &&
         form.value.terms
})

function validate() {
  errors.value = {}
  
  if (!form.value.firstName.trim()) {
    errors.value.firstName = 'First name is required'
  }
  if (!form.value.lastName.trim()) {
    errors.value.lastName = 'Last name is required'
  }
  if (!form.value.email.includes('@')) {
    errors.value.email = 'Valid email is required'
  }
  if (!form.value.terms) {
    errors.value.terms = 'You must accept the terms'
  }
}

function submit() {
  validate()
  if (isValid.value) {
    console.log('Form submitted:', form.value)
  }
}
</script>

<template>
  <form @submit.prevent="submit">
    <h2>Registration Form</h2>
    
    <div class="form-group">
      <label for="firstName">First Name *</label>
      <input 
        id="firstName" 
        v-model="form.firstName" 
        type="text"
        :class="{ error: errors.firstName }"
      >
      <span v-if="errors.firstName" class="error-message">{{ errors.firstName }}</span>
    </div>

    <div class="form-group">
      <label for="lastName">Last Name *</label>
      <input 
        id="lastName" 
        v-model="form.lastName" 
        type="text"
        :class="{ error: errors.lastName }"
      >
      <span v-if="errors.lastName" class="error-message">{{ errors.lastName }}</span>
    </div>

    <div class="form-group">
      <label for="email">Email *</label>
      <input 
        id="email" 
        v-model="form.email" 
        type="email"
        :class="{ error: errors.email }"
      >
      <span v-if="errors.email" class="error-message">{{ errors.email }}</span>
    </div>

    <div class="form-group">
      <label for="phone">Phone</label>
      <input 
        id="phone" 
        v-model="form.phone" 
        type="tel"
      >
    </div>

    <div class="form-group">
      <label for="address">Address</label>
      <input 
        id="address" 
        v-model="form.address" 
        type="text"
      >
    </div>

    <div class="form-row">
      <div class="form-group">
        <label for="city">City</label>
        <input 
          id="city" 
          v-model="form.city" 
          type="text"
        >
      </div>

      <div class="form-group">
        <label for="state">State</label>
        <input 
          id="state" 
          v-model="form.state" 
          type="text"
        >
      </div>

      <div class="form-group">
        <label for="zip">ZIP</label>
        <input 
          id="zip" 
          v-model="form.zip" 
          type="text"
        >
      </div>
    </div>

    <div class="form-group">
      <label for="country">Country</label>
      <select id="country" v-model="form.country">
        <option value="US">United States</option>
        <option value="CA">Canada</option>
        <option value="MX">Mexico</option>
        <option value="UK">United Kingdom</option>
        <option value="FR">France</option>
        <option value="DE">Germany</option>
      </select>
    </div>

    <div class="form-group">
      <label>
        <input type="checkbox" v-model="form.newsletter">
        Subscribe to newsletter
      </label>
    </div>

    <div class="form-group">
      <label>
        <input type="checkbox" v-model="form.terms">
        I accept the terms and conditions *
      </label>
      <span v-if="errors.terms" class="error-message">{{ errors.terms }}</span>
    </div>

    <button type="submit" :disabled="!isValid">Submit</button>
  </form>
</template>

<style scoped>
.form-group {
  margin-bottom: 1rem;
}

.form-row {
  display: flex;
  gap: 1rem;
}

.error {
  border-color: red;
}

.error-message {
  color: red;
  font-size: 0.875rem;
}
</style>
