import path from 'node:path';
import tailwindcss from '@tailwindcss/vite';
import react from '@vitejs/plugin-react';
import { defineConfig } from 'vite';
import { VitePWA } from 'vite-plugin-pwa';

export default defineConfig({
  plugins: [
    tailwindcss(),
    react(),
    VitePWA({
      registerType: 'autoUpdate',
      includeAssets: ['favicon.png', 'logo.png', 'icons/*.png'],
      manifest: {
        name: 'Pulpo',
        short_name: 'Pulpo',
        description: 'Agent session orchestrator — manage coding agents across your machines',
        theme_color: '#081a33',
        background_color: '#081a33',
        display: 'standalone',
        orientation: 'any',
        start_url: '/',
        scope: '/',
        icons: [
          {
            src: '/icons/icon-192x192.png',
            sizes: '192x192',
            type: 'image/png',
          },
          {
            src: '/icons/icon-512x512.png',
            sizes: '512x512',
            type: 'image/png',
          },
          {
            src: '/icons/icon-512x512.png',
            sizes: '512x512',
            type: 'image/png',
            purpose: 'maskable',
          },
        ],
      },
      workbox: {
        // Cache the app shell (HTML, JS, CSS) — exclude sprites (loaded via runtime cache)
        globPatterns: ['**/*.{js,css,html,woff2}', 'favicon.png', 'icons/*.png'],
        // Network-first for API calls — always try live data
        runtimeCaching: [
          {
            urlPattern: /^\/api\//,
            handler: 'NetworkOnly',
          },
          {
            // Cache sprite images (they don't change often)
            urlPattern: /\/sprites\//,
            handler: 'CacheFirst',
            options: {
              cacheName: 'sprites',
              expiration: {
                maxEntries: 100,
                maxAgeSeconds: 60 * 60 * 24 * 30, // 30 days
              },
            },
          },
        ],
      },
    }),
  ],
  resolve: {
    alias: {
      '@': path.resolve(__dirname, './src'),
    },
  },
  build: {
    outDir: 'build',
  },
  server: {
    proxy: {
      '/api': {
        target: 'http://localhost:7433',
        ws: true,
      },
    },
  },
});
