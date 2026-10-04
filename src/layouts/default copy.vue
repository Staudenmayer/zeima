<template>
  <div class="app">
    <header class="pa-5">
        <nav>
          <RouterLink to="/" class="pe-5">Go to Home</RouterLink>
          <RouterLink to="/about">Go to About</RouterLink>
        </nav>
        <div>{{ $route.fullPath }}</div>
    </header>

    <main>
      <slot />
    </main>
  </div>
</template>