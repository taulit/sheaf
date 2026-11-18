// @ts-check
import { defineConfig } from 'astro/config';
import starlight from '@astrojs/starlight';
import sitemap from '@astrojs/sitemap';

const site = 'https://taulit.github.io';
const base = '/sheaf';
const ogImage = `${site}${base}/og.png`;

export default defineConfig({
  site,
  base,
  trailingSlash: 'always',
  integrations: [
    starlight({
      title: 'Sheaf',
      description:
        'Sheaf is a fast, native notes app for GNOME. Apple Notes–style gallery, editing in place, search in milliseconds, and every note is a plain Markdown file.',
      logo: { src: './src/assets/icon.svg', alt: 'Sheaf' },
      favicon: '/favicon.svg',
      social: [{ icon: 'github', label: 'GitHub', href: 'https://github.com/taulit/sheaf' }],
      editLink: { baseUrl: 'https://github.com/taulit/sheaf/edit/main/site/' },
      lastUpdated: true,
      customCss: ['./src/styles/custom.css'],
      head: [
        { tag: 'meta', attrs: { property: 'og:image', content: ogImage } },
        { tag: 'meta', attrs: { property: 'og:image:width', content: '1200' } },
        { tag: 'meta', attrs: { property: 'og:image:height', content: '630' } },
        { tag: 'meta', attrs: { property: 'og:image:alt', content: 'Sheaf, fast native notes for GNOME, with a gallery of note cards' } },
        { tag: 'meta', attrs: { name: 'twitter:card', content: 'summary_large_image' } },
        { tag: 'meta', attrs: { name: 'twitter:image', content: ogImage } },
        { tag: 'meta', attrs: { name: 'theme-color', content: '#3584e4' } },
      ],
      sidebar: [
        {
          label: 'Guide',
          items: [
            { label: 'Install', slug: 'install' },
            { label: 'Using Sheaf', slug: 'usage' },
            { label: 'Search', slug: 'search' },
            { label: 'Keyboard shortcuts', slug: 'shortcuts' },
            { label: 'Your notes folder', slug: 'data-format' },
            { label: 'Command line', slug: 'cli' },
            { label: 'FAQ', slug: 'faq' },
          ],
        },
        {
          label: 'Related projects',
          items: [
            { label: 'gpui-markdown-editor', link: 'https://taulit.github.io/gpui-markdown-editor/', attrs: { target: '_blank' } },
            { label: 'Visor (GNOME notch)', link: 'https://github.com/taulit/visor', attrs: { target: '_blank' } },
          ],
        },
      ],
    }),
    sitemap(),
  ],
});
