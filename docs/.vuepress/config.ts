import { viteBundler } from '@vuepress/bundler-vite';
import { defaultTheme } from '@vuepress/theme-default';
import { defineUserConfig } from 'vuepress';

export default defineUserConfig({
  lang: 'en-US',
  title: 'Pulpo Docs',
  description: 'Self-hosted control plane docs for coding agents',
  base: '/pulpo/',
  theme: defaultTheme({
    logo: 'https://raw.githubusercontent.com/darioblanco/pulpo/main/web/public/logo.svg',
    repo: 'darioblanco/pulpo',
    docsDir: 'docs',
    navbar: [
      { text: 'Getting Started', link: '/getting-started/install' },
      { text: 'Guides', link: '/guides/configuration' },
      { text: 'Reference', link: '/reference/cli' },
      { text: 'Architecture', link: '/architecture/overview' },
      { text: 'Operations', link: '/operations/release-and-distribution' },
    ],
    sidebar: {
      '/getting-started/': [
        '/getting-started/install',
        '/getting-started/quickstart',
      ],
      '/guides/': [
        '/guides/configuration',
        '/guides/discovery',
        '/guides/recovery',
      ],
      '/reference/': [
        '/reference/cli',
        '/reference/config',
        '/reference/api',
      ],
      '/architecture/': ['/architecture/overview'],
      '/operations/': ['/operations/release-and-distribution'],
    },
  }),
  bundler: viteBundler(),
});
