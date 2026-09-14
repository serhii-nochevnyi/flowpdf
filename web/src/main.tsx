import { createRoot } from 'react-dom/client'

import { EditorApp, type EditorLocale } from './editor/editor-app.js'

const locale: EditorLocale = document.documentElement.lang.toLowerCase().startsWith('en')
  ? 'en'
  : 'uk'
const root = document.querySelector<HTMLElement>('#app')

if (root === null) {
  throw new Error('FLOW_EDITOR_ROOT_MISSING')
}

document.title =
  locale === 'uk'
    ? 'FlowPDF — Семантичний редактор'
    : 'FlowPDF — Semantic editor'

createRoot(root).render(<EditorApp options={{ locale }} />)
