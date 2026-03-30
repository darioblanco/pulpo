import { viteBundler } from '@vuepress/bundler-vite';
import { defaultTheme } from '@vuepress/theme-default';
import { defineUserConfig } from 'vuepress';

export default defineUserConfig({
  lang: 'en-US',
  title: 'Pulpo Docs',
  description: 'Self-hosted control plane for coding agents',
  base: '/',
  head: [
    [
      'link',
      {
        rel: 'icon',
        href: 'https://raw.githubusercontent.com/darioblanco/pulpo/main/web/public/logo.png',
      },
    ],
  ],
  theme: defaultTheme({
    logo: 'https://raw.githubusercontent.com/darioblanco/pulpo/main/web/public/logo.png',
    repo: 'darioblanco/pulpo',
    docsDir: 'docs',
    colorMode: 'auto',
    colorModeSwitch: true,
    navbar: [
      { text: 'Why Pulpo', link: '/getting-started/why-pulpo' },
      { text: 'Getting Started', link: '/getting-started/install' },
      { text: 'Core Concepts', link: '/architecture/core-concepts' },
      { text: 'Guides', link: '/guides/configuration' },
      { text: 'Reference', link: '/reference/cli' },
      { text: 'Architecture', link: '/architecture/overview' },
      { text: 'Operations', link: '/operations/session-lifecycle' },
    ],
    sidebar: {
      '/getting-started/': [
        '/getting-started/why-pulpo',
        '/getting-started/use-cases',
        '/getting-started/alternatives',
        '/getting-started/install',
        '/getting-started/quickstart',
      ],
      '/architecture/': ['/architecture/core-concepts', '/architecture/overview'],
      '/guides/': [
        '/guides/nightly-code-review',
        '/guides/parallel-agents-one-repo',
        '/guides/private-infra-with-tailscale',
        '/guides/docker-isolated-risky-tasks',
        '/guides/configuration',
        '/guides/discovery',
        '/guides/recovery',
        '/guides/worktrees',
        '/guides/secrets',
      ],
      '/reference/': ['/reference/cli', '/reference/config', '/reference/api'],
      '/operations/': ['/operations/session-lifecycle', '/operations/release-and-distribution'],
    },
  }),
  bundler: viteBundler(),
});
