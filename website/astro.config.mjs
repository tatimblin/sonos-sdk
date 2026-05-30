import { defineConfig } from 'astro/config';
import starlight from '@astrojs/starlight';

export default defineConfig({
  site: 'https://tatimblin.github.io',
  base: '/sonos-sdk',
  integrations: [
    starlight({
      title: 'Sonos SDK',
      components: {
        Header: './src/components/Header.astro',
        SiteTitle: './src/components/SiteTitle.astro',
      },
      customCss: ['./src/styles/custom.css'],
      editLink: {
        baseUrl: 'https://github.com/tatimblin/sonos-sdk/edit/main/website/',
      },
      sidebar: [
        {
          label: 'Getting Started',
          items: [
            { slug: 'getting-started/installation' },
            { slug: 'getting-started/quick-start' },
          ],
        },
        {
          label: 'Guides',
          items: [
            { slug: 'guides/architecture' },
            { slug: 'guides/properties' },
            { slug: 'guides/playback' },
            { slug: 'guides/volume-and-eq' },
            { slug: 'guides/queue' },
            { slug: 'guides/groups' },
            { slug: 'guides/timers' },
          ],
        },
        {
          label: 'Cookbook',
          items: [{ autogenerate: { directory: 'guides/cookbook' } }],
        },
        {
          label: 'CLI',
          items: [
            { slug: 'cli' },
            { slug: 'cli/commands' },
          ],
        },
        {
          label: 'Troubleshooting',
          items: [{ autogenerate: { directory: 'troubleshooting' } }],
        },
      ],
    }),
  ],
});
