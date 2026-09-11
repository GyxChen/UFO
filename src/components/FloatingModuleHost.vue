<script setup lang="ts">
import { computed } from 'vue'
import AgentStatusModule from './AgentStatusModule.vue'
import type { FloatingLayout } from '@/lib/floating-modules'
import { createFloatingModuleDefinitions, enabledModuleDefinitions } from '@/lib/floating-modules'
import { useFloatingModulesStore } from '@/stores/floatingModules'

const props = defineProps<{ layoutMode: FloatingLayout }>()
const modules = useFloatingModulesStore()
const definitions = createFloatingModuleDefinitions(AgentStatusModule)
const enabledModules = computed(() => enabledModuleDefinitions(modules.config, definitions))
</script>

<template>
  <div class="floating-module-host" :class="`floating-module-host--${props.layoutMode}`">
    <div class="floating-module-host__slot floating-module-host__slot--status">
      <component
        :is="definition.component"
        v-for="definition in enabledModules.slice(0, 1)"
        :key="definition.id"
        :layout-mode="props.layoutMode"
      />
    </div>
  </div>
</template>
