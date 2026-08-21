import { mountFoundationInspector } from './foundation-inspector.js'

const root = document.querySelector<HTMLElement>('#app')
if (root === null) {
  throw new Error('FLOW_INSPECTOR_ROOT_MISSING')
}

try {
  await mountFoundationInspector(root)
} catch {
  const alert = document.createElement('p')
  alert.className = 'startup-error'
  alert.setAttribute('role', 'alert')
  alert.textContent = 'Не вдалося запустити локальний інспектор. Код: FLOW_STARTUP_FAILED.'
  root.replaceChildren(alert)
}
