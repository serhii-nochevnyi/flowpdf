import { resolve } from 'node:path'

import react from '@vitejs/plugin-react'
import { defineConfig } from 'vite'

export default defineConfig({
  root: 'web',
  publicDir: false,
  plugins: [
    {
      name: 'flowpdf-foundation-inspector-entry',
      transformIndexHtml: {
        order: 'pre',
        handler(html) {
          return html.replace('href="/styles.css"', 'href="/src/styles.css"')
        },
      },
    },
    react(),
  ],
  server: {
    fs: {
      strict: true,
    },
  },
  build: {
    outDir: '../dist/web',
    emptyOutDir: false,
    rolldownOptions: {
      input: {
        inspector: resolve(import.meta.dirname, 'web/index.html'),
        editor: resolve(import.meta.dirname, 'web/editor.html'),
      },
    },
  },
})
