import path from 'node:path';
import tailwindcss from '@tailwindcss/vite';
import { svelte } from '@sveltejs/vite-plugin-svelte';
import { defineConfig } from 'vitest/config';

export default defineConfig({
  plugins: [tailwindcss(), svelte({ hot: false })],
  resolve: {
    alias: {
      $lib: path.resolve(__dirname, 'src/lib'),
      '$app/navigation': path.resolve(__dirname, 'src/lib/__mocks__/app-navigation.ts'),
      '$app/state': path.resolve(__dirname, 'src/lib/__mocks__/app-state.ts'),
      '@tauri-apps/api/core': path.resolve(__dirname, 'src/lib/__mocks__/tauri-api-core.ts'),
    },
    conditions: ['browser'],
  },
  test: {
    environment: 'jsdom',
    include: ['src/**/*.{test,spec}.{js,ts}'],
    coverage: {
      provider: 'v8',
      include: ['src/**/*.{ts,svelte}'],
      exclude: [
        'src/**/*.{test,spec}.{js,ts}',
        'src/**/*.d.ts',
        'src/app.css',
        'src/lib/index.ts',
        'src/lib/__mocks__/**',
        'src/routes/+layout.svelte',
        'src/routes/+layout.ts',
      ],
      thresholds: {
        lines: 100,
      },
    },
  },
});
