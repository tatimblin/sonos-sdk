import { defineConfig } from 'astro/config';
import starlight from '@astrojs/starlight';

export default defineConfig({
  site: 'https://tatimblin.github.io',
  base: '/sonos-sdk',
  integrations: [
    starlight({
      title: 'Sonos SDK',
      social: [
        { icon: 'github', label: 'GitHub', href: 'https://github.com/tatimblin/sonos-sdk' },
      ],
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
            {
              label: 'Cookbook',
              items: [{ autogenerate: { directory: 'guides/cookbook' } }],
            },
          ],
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
