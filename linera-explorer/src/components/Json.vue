<script setup lang="ts" >
import { onMounted } from 'vue'
import JSONFormatter from 'json-formatter-js'

function json_load(id: string, data: any) {
  let formatter = new JSONFormatter(data, Infinity)
  let elt = document.getElementById(id)
  elt!.appendChild(formatter.render())
}

const props = defineProps<{id?: string, data: any}>()
const inner_id =
  props.id ? props.id + '-inner' :
  Date.now() + Math.floor(Math.random() * 100) + '-inner'

onMounted(() => {
  json_load(inner_id, props.data)
})
</script>

<template>
  <div :id="inner_id" style="overflow-x: auto"></div>
</template>
