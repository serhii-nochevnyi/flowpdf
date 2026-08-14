import { expect, test } from 'vitest'

test('runs inside the named Chromium project', () => {
  const marker = document.createElement('main')
  marker.textContent = 'FlowPDF'
  document.body.append(marker)

  expect(marker.textContent).toBe('FlowPDF')
  expect(navigator.userAgent).toContain('HeadlessChrome')
})

