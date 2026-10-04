<template>
  <SidebarProvider>
    <AppSidebar />
    <SidebarInset>
      <header class="flex h-16 shrink-0 items-center gap-2 border-b px-4">
        <SidebarTrigger class="-ml-1" />
        <Separator
          orientation="vertical"
          class="mr-2 data-[orientation=vertical]:h-4"
        />
        <Breadcrumb>
          <BreadcrumbList>
            <template
              v-for="(match, index) in $route.matched"
              :key="match.path"
            >
              <BreadcrumbItem
                :class="{ 'hidden md:block': index < $route.matched.length - 1 }"
              >
                <BreadcrumbLink
                  v-if="index < $route.matched.length - 1"
                  :as-child="true"
                >
                  <RouterLink :to="match.path">
                    {{ match.meta.breadcrumb }}
                  </RouterLink>
                </BreadcrumbLink>

                <BreadcrumbPage v-else>
                  {{ match.meta.breadcrumb }}
                </BreadcrumbPage>
              </BreadcrumbItem>

              <BreadcrumbSeparator
                v-if="index < $route.matched.length - 1"
                class="hidden md:block"
              />
            </template>
          </BreadcrumbList>
        </Breadcrumb>
        <ThemeSwitch class="ml-auto" />
      </header>
      <main>
        <slot />
      </main>
    </SidebarInset>
  </SidebarProvider>
</template>

<script setup lang="ts">
import AppSidebar from "@/components/AppSidebar.vue"
import ThemeSwitch from "@/components/ThemeSwitch.vue";
import {
  Breadcrumb,
  BreadcrumbItem,
  BreadcrumbLink,
  BreadcrumbList,
  BreadcrumbPage,
  BreadcrumbSeparator,
} from "@/components/ui/breadcrumb"
import { Separator } from "@/components/ui/separator"
import {
  SidebarInset,
  SidebarProvider,
  SidebarTrigger,
} from "@/components/ui/sidebar"
</script>
