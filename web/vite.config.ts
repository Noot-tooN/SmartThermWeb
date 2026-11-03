import { defineConfig } from 'vitest/config';
import { svelte } from '@sveltejs/vite-plugin-svelte';
export default defineConfig({
  base: './',
  plugins: [svelte()],
  server: { proxy: { '/api': 'http://127.0.0.1:3000' } },
  test: {
    environment: 'jsdom',
    include: ['src/**/*.test.ts'],
    restoreMocks: true,
  },
  resolve: { conditions: ['browser'] },
});
