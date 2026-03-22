import path from 'node:path';
import tailwindcss from '@tailwindcss/vite';
import react from '@vitejs/plugin-react';
import { defineConfig } from 'vitest/config';

export default defineConfig({
  plugins: [tailwindcss(), react()],
  resolve: {
    alias: {
      '@': path.resolve(__dirname, './src'),
    },
  },
  test: {
    environment: 'jsdom',
    pool: 'threads',
    include: ['src/**/*.{test,spec}.{js,ts,tsx}'],
    setupFiles: ['src/test-setup.ts'],
    coverage: {
      provider: 'v8',
      include: ['src/**/*.{ts,tsx}'],
      exclude: [
        'src/**/*.{test,spec}.{js,ts,tsx}',
        'src/**/*.d.ts',
        'src/index.css',
        'src/main.tsx',
        'src/sw.ts',
        'src/test-setup.ts',
        'src/vite-env.d.ts',
        'src/components/ui/**',
        'src/hooks/use-mobile.ts',
        'src/api/types.ts',
      ],
      thresholds: {
        lines: 100,
      },
    },
  },
});
