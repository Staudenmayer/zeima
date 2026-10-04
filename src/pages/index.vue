<template>
    <div class="p-5">
        <div class="p-5 text-center">
            <Button @click="count++">Count: {{ count }}</Button>
        </div>
        <BarChart/>
    </div>
</template>
<script setup lang="ts">
import { ref } from 'vue';
import Button from '@/components/ui/button/Button.vue';
import BarChart from '@/components/BarChart.vue';

const count = ref<number>(0);

definePage({
    meta: {
        breadcrumb: 'Home'
    }
})
</script>