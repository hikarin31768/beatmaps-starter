<script setup>
import { useId } from 'vue'

defineProps({ disabled: Boolean })

const id = useId()
const leadIn = defineModel('leadIn', { type: [Number, String] })
const leadInUnit = defineModel('leadInUnit', { type: String })
const tailOffset = defineModel('tailOffset', { type: [Number, String] })
const tailOffsetUnit = defineModel('tailOffsetUnit', { type: String })
</script>

<template>
  <div class="offset-defaults">
    <div class="offset-default">
      <label :for="id + '-front'">默认音频前偏移</label>
      <div class="offset-value-unit">
        <input :id="id + '-front'" v-model.number="leadIn" type="number" min="0" step="any" required :disabled="disabled" />
        <select v-model="leadInUnit" aria-label="前偏移单位" :disabled="disabled">
          <option value="seconds">秒</option>
          <option value="beats">拍</option>
        </select>
      </div>
    </div>
    <div class="offset-default">
      <label :for="id + '-tail'">默认音频后偏移</label>
      <div class="offset-value-unit">
        <input :id="id + '-tail'" v-model.number="tailOffset" type="number" min="0" step="any" required :disabled="disabled" />
        <select v-model="tailOffsetUnit" aria-label="后偏移单位" :disabled="disabled">
          <option value="seconds">秒</option>
          <option value="beats">拍</option>
        </select>
      </div>
    </div>
  </div>
</template>
