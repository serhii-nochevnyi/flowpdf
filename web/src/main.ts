import {
  foundationInspectorMessages,
  mountFoundationInspector,
  type FoundationInspectorLocale,
} from './foundation-inspector.js'

const locale: FoundationInspectorLocale = document.documentElement.lang
  .toLowerCase()
  .startsWith('en')
  ? 'en'
  : 'uk'
const copy = foundationInspectorMessages[locale]

document.title = copy['foundationInspector.documentTitle']

const root = document.querySelector<HTMLElement>('#app')
if (root === null) {
  throw new Error('FLOW_INSPECTOR_ROOT_MISSING')
}

try {
  await mountFoundationInspector(root, { locale })
} catch {
  const alert = document.createElement('p')
  alert.className = 'startup-error'
  alert.setAttribute('role', 'alert')
  alert.textContent = copy['foundationInspector.error.startup']
  root.replaceChildren(alert)
}
