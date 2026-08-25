import { page } from 'vitest/browser'
import { expect, test, vi } from 'vitest'

import {
  mountFoundationInspector,
  type FoundationInspectorController,
  type FoundationInspectorLocale,
} from '../src/foundation-inspector'
import '../src/styles.css'

interface LocaleExpectation {
  readonly create: string
  readonly inspector: string
  readonly unavailableProvenance: RegExp
}

const localeExpectations: Readonly<Record<FoundationInspectorLocale, LocaleExpectation>> = {
  uk: {
    create: 'Створити тестовий документ',
    inspector: 'Інспектор документа',
    unavailableProvenance: /наступній фазі/i,
  },
  en: {
    create: 'Create sample document',
    inspector: 'Document inspector',
    unavailableProvenance: /next phase/i,
  },
}

for (const locale of ['uk', 'en'] as const) {
  for (const width of [320, 1280] as const) {
    test(`accessibility: ${locale} lifecycle remains semantic and unclipped at ${width}px`, async () => {
      await page.viewport(width, 900)
      const root = document.createElement('div')
      document.body.replaceChildren(root)
      const copy = localeExpectations[locale]
      const inspector = await mountFoundationInspector(root, {
        databaseName: `flowpdf-accessibility-${locale}-${width}-${crypto.randomUUID()}`,
        locale,
      })

      expect(window.innerWidth).toBe(width)
      expect(root.lang).toBe(locale)
      expect(root.querySelectorAll('header')).toHaveLength(1)
      expect(root.querySelectorAll('main')).toHaveLength(1)
      expect(root.querySelector('[aria-labelledby="commands-heading"]')).not.toBeNull()
      expect(root.querySelector('aside')?.getAttribute('aria-label')).toBe(copy.inspector)
      expect(root.querySelector('[aria-labelledby="audit-heading"]')).not.toBeNull()
      expect(action(root, 'create-sample').textContent).toBe(copy.create)
      expect(action(root, 'undo').disabled).toBe(true)
      expect(action(root, 'redo').disabled).toBe(true)
      expect(root.querySelector('[aria-live="polite"][data-durability-status]')).not.toBeNull()
      const copyStatus = root.querySelector<HTMLElement>('[data-copy-status]')
      expect(copyStatus?.getAttribute('role')).toBe('status')
      expect(copyStatus?.getAttribute('aria-live')).toBe('polite')
      expect(copyStatus?.getAttribute('aria-atomic')).toBe('true')
      const copyAlert = root.querySelector<HTMLElement>('[data-copy-alert]')
      expect(copyAlert?.getAttribute('role')).toBe('alert')
      expect(copyAlert?.getAttribute('aria-atomic')).toBe('true')
      const lifecycleAlert = root.querySelector<HTMLElement>(
        '.alert-region[role="alert"]',
      )
      expect(lifecycleAlert).not.toBeNull()
      expect(root.querySelector('[role="alert"]')).toBe(lifecycleAlert)
      expect(copyAlert).not.toBe(lifecycleAlert)
      expect(root.querySelector('[data-provenance-unavailable]')?.textContent).toMatch(
        copy.unavailableProvenance,
      )

      const create = action(root, 'create-sample')
      create.focus()
      const focusStyle = getComputedStyle(create)
      expect(focusStyle.outlineStyle).toBe('solid')
      expect(focusStyle.outlineWidth).toBe('3px')
      expect(focusStyle.outlineOffset).toBe('2px')
      expect(
        getComputedStyle(action(root, 'open-older-schema')).borderTopColor,
      ).toBe('rgb(71, 85, 105)')
      const emptyHeadingStyle = getComputedStyle(
        requiredElement(root, '[data-empty-document] h3'),
      )
      expect(emptyHeadingStyle.fontSize).toBe('20px')
      expect(emptyHeadingStyle.fontWeight).toBe('600')
      assertTargetSizes(root)
      assertResponsiveColumns(root, width)
      assertNoViewportOverflow(root, width)

      await clickAndWait(inspector, root, 'create-sample')
      const created = inspector.snapshot()
      expect(document.activeElement).toBe(action(root, 'apply-mutation'))
      expect(root.querySelector('[data-durability-status]')?.textContent).not.toBe('')
      expect(root.querySelector('[data-audit-count]')?.getAttribute('aria-live')).toBe(
        'polite',
      )
      expect(root.querySelectorAll('table[data-audit] th[scope="col"]')).toHaveLength(6)
      expect(root.querySelectorAll('[data-audit-row]')).toHaveLength(1)
      expect(root.querySelector('time[datetime]')).not.toBeNull()
      expect(action(root, 'undo').disabled).toBe(true)
      expect(getComputedStyle(requiredElement(root, '.command-group-heading')).fontSize).toBe(
        '14px',
      )
      assertAuditRecordsFit(root)

      const fullDocumentId = root.querySelector<HTMLElement>('[data-full-document-id]')
      const fullHash = root.querySelector<HTMLElement>('[data-full-revision-hash]')
      const idCopy = root.querySelector<HTMLButtonElement>('[data-copy="document-id"]')
      const hashCopy = root.querySelector<HTMLButtonElement>('[data-copy="revision-hash"]')
      expect(fullDocumentId?.textContent).toBe(created.documentId)
      expect(fullHash?.textContent).toBe(created.hash)
      expect(fullHash?.getAttribute('aria-label')).toBe(created.hash)
      expect(hashCopy?.getAttribute('aria-label')).toContain(created.hash)

      const clipboardWrite = vi
        .spyOn(navigator.clipboard, 'writeText')
        .mockResolvedValue(undefined)
      try {
        idCopy?.click()
        await waitUntil(() => copyStatus?.textContent.trim() !== '')
        expect(clipboardWrite).toHaveBeenCalledWith(created.documentId)
        assertVisibleFeedback(copyStatus)

        clipboardWrite.mockRejectedValueOnce(new Error('clipboard denied'))
        hashCopy?.click()
        await waitUntil(() => copyAlert?.textContent.includes('FLOW_CLIPBOARD_WRITE_FAILED') === true)
        expect(copyStatus?.textContent).toBe('')
        assertVisibleFeedback(copyAlert)
        expect(lifecycleAlert?.hidden).toBe(true)
      } finally {
        clipboardWrite.mockRestore()
      }

      await clickAndWait(inspector, root, 'apply-mutation')
      await clickAndWait(inspector, root, 'apply-mutation')
      expect(action(root, 'undo').disabled).toBe(false)
      expect(action(root, 'redo').disabled).toBe(true)
      expect(focusableControlOrder(root)).toEqual([
        'apply-mutation',
        'stale-command',
        'undo',
        'save',
        'reload',
        'recover',
        'document-id',
        'revision-hash',
      ])

      const undo = action(root, 'undo')
      undo.focus()
      const undoShortcut = new KeyboardEvent('keydown', {
        key: 'z',
        ctrlKey: true,
        bubbles: true,
        cancelable: true,
      })
      undo.dispatchEvent(undoShortcut)
      await inspector.whenIdle()
      expect(undoShortcut.defaultPrevented).toBe(true)
      expect(document.activeElement).toBe(undo)
      expect(inspector.snapshot().audit.at(-1)?.action).toEqual({
        type: 'command',
        commandKind: 'undo',
      })
      expect(action(root, 'redo').disabled).toBe(false)

      undo.dispatchEvent(
        new KeyboardEvent('keydown', {
          key: 'z',
          ctrlKey: true,
          bubbles: true,
          cancelable: true,
        }),
      )
      await inspector.whenIdle()
      expect(action(root, 'undo').disabled).toBe(true)
      expect(document.activeElement).toBe(action(root, 'redo'))

      const redo = action(root, 'redo')
      const redoShortcut = new KeyboardEvent('keydown', {
        key: 'z',
        ctrlKey: true,
        shiftKey: true,
        bubbles: true,
        cancelable: true,
      })
      redo.dispatchEvent(redoShortcut)
      await inspector.whenIdle()
      expect(redoShortcut.defaultPrevented).toBe(true)
      expect(action(root, 'redo').disabled).toBe(false)
      expect(document.activeElement).toBe(redo)

      redo.dispatchEvent(
        new KeyboardEvent('keydown', {
          key: 'z',
          ctrlKey: true,
          shiftKey: true,
          bubbles: true,
          cancelable: true,
        }),
      )
      await inspector.whenIdle()
      expect(action(root, 'redo').disabled).toBe(true)
      expect(document.activeElement).toBe(undo)
      expect(inspector.snapshot().audit.at(-1)?.action).toEqual({
        type: 'command',
        commandKind: 'redo',
      })

      const beforeConflict = inspector.snapshot()
      await clickAndWait(inspector, root, 'stale-command')
      const rejected = inspector.snapshot()
      expect(rejected.revision).toBe(beforeConflict.revision)
      expect(rejected.hash).toBe(beforeConflict.hash)
      expect(rejected.audit).toHaveLength(beforeConflict.audit.length + 1)
      expect(rejected.audit.at(-1)).toMatchObject({
        baseRevision: beforeConflict.revision,
        newRevision: beforeConflict.revision,
        action: { type: 'command', commandKind: 'insertText' },
        outcome: { kind: 'failure', code: 'staleRevision' },
      })
      expect(root.querySelector('.alert-region[role="alert"]')?.textContent).toContain(
        'FLOW_STALE_REVISION',
      )
      expect(document.activeElement).toBe(action(root, 'stale-command'))

      const audit = root.querySelector<HTMLElement>('[aria-labelledby="audit-heading"]')
      expect(audit?.textContent).not.toMatch(
        /Український|English sample|typed mutation|rawTranscript|rawAudio|commandArguments|canonicalJson|storage payload/i,
      )
      expect(root.textContent).not.toMatch(
        /Український|English sample|typed mutation|rawTranscript|rawAudio|commandArguments|canonicalJson/i,
      )
      expect(
        [...root.querySelectorAll<HTMLElement>('[data-audit-row]')].map(
          ({ dataset }) => dataset.sourceIndex,
        ),
      ).toEqual(Array.from({ length: rejected.audit.length }, (_, index) => String(index)))
      expect(
        new Set(
          [...root.querySelectorAll<HTMLElement>('[data-audit-row]')].map(
            ({ dataset }) => dataset.auditId,
          ),
        ).size,
      ).toBe(rejected.audit.length)

      assertTargetSizes(root)
      assertMinimumTextSize(root)
      assertResponsiveColumns(root, width)
      assertNoViewportOverflow(root, width)
      assertAuditRecordsFit(root)
    })
  }
}

function action(root: HTMLElement, name: string): HTMLButtonElement {
  const control = root.querySelector<HTMLButtonElement>(`[data-action="${name}"]`)
  if (control === null) throw new Error(`missing ${name} control`)
  return control
}

async function clickAndWait(
  inspector: FoundationInspectorController,
  root: HTMLElement,
  name: string,
): Promise<void> {
  action(root, name).click()
  await inspector.whenIdle()
}

async function waitUntil(predicate: () => boolean): Promise<void> {
  for (let attempt = 0; attempt < 20; attempt += 1) {
    if (predicate()) return
    await new Promise((resolve) => setTimeout(resolve, 0))
  }
  throw new Error('condition was not reached')
}

function focusableControlOrder(root: HTMLElement): string[] {
  return [...root.querySelectorAll<HTMLButtonElement>('button:not([disabled]):not([hidden])')]
    .filter(isVisible)
    .map((control) => {
      expect(control.tabIndex).toBe(0)
      expect(accessibleName(control)).not.toBe('')
      return control.dataset.action ?? control.dataset.copy ?? 'unknown-control'
    })
}

function assertTargetSizes(root: HTMLElement): void {
  for (const control of root.querySelectorAll<HTMLButtonElement>('button')) {
    if (!isVisible(control)) continue
    const bounds = control.getBoundingClientRect()
    expect(bounds.width).toBeGreaterThanOrEqual(44)
    expect(bounds.height).toBeGreaterThanOrEqual(44)
    expect(accessibleName(control)).not.toBe('')
  }
}

function assertVisibleFeedback(node: HTMLElement | null): void {
  expect(node?.textContent.trim()).not.toBe('')
  if (node === null) throw new Error('missing copy feedback')
  const style = getComputedStyle(node)
  expect(style.display).not.toBe('none')
  expect(style.visibility).toBe('visible')
  expect(Number.parseFloat(style.opacity)).toBeGreaterThan(0)
  const bounds = node.getBoundingClientRect()
  expect(bounds.width).toBeGreaterThan(0)
  expect(bounds.height).toBeGreaterThan(0)
}

function assertMinimumTextSize(root: HTMLElement): void {
  for (const node of root.querySelectorAll<HTMLElement>('h1, h2, h3, p, dt, dd, th, td, button')) {
    if (!isVisible(node) || node.textContent?.trim() === '') continue
    expect(Number.parseFloat(getComputedStyle(node).fontSize)).toBeGreaterThanOrEqual(14)
  }
}

function assertResponsiveColumns(root: HTMLElement, width: number): void {
  const primary = requiredElement(root, '.primary-column').getBoundingClientRect()
  const inspector = requiredElement(root, 'aside').getBoundingClientRect()
  if (width >= 960) {
    expect(primary.right).toBeLessThanOrEqual(inspector.left)
  } else {
    expect(primary.bottom).toBeLessThanOrEqual(inspector.top)
  }
}

function assertNoViewportOverflow(root: HTMLElement, width: number): void {
  expect(document.documentElement.scrollWidth).toBeLessThanOrEqual(width)
  for (const node of root.querySelectorAll<HTMLElement>(
    'button, h1, h2, h3, [role="alert"], [data-durability-status]',
  )) {
    if (!isVisible(node)) continue
    const bounds = node.getBoundingClientRect()
    expect(bounds.left).toBeGreaterThanOrEqual(0)
    expect(bounds.right).toBeLessThanOrEqual(width)
  }
}

function assertAuditRecordsFit(root: HTMLElement): void {
  const wrapper = requiredElement(root, '.audit-table-wrapper')
  expect(wrapper.scrollWidth).toBeLessThanOrEqual(wrapper.clientWidth)
  const headingIds = new Set(
    [...root.querySelectorAll<HTMLElement>('table[data-audit] th[scope="col"]')].map(
      ({ id }) => id,
    ),
  )
  for (const row of root.querySelectorAll<HTMLElement>('[data-audit-row]')) {
    const cells = [...row.querySelectorAll<HTMLTableCellElement>('td')]
    expect(cells).toHaveLength(6)
    for (const cell of cells) {
      expect(cell.dataset.label?.trim()).not.toBe('')
      expect(headingIds.has(cell.headers)).toBe(true)
    }
  }
}

function requiredElement(root: HTMLElement, selector: string): HTMLElement {
  const node = root.querySelector<HTMLElement>(selector)
  if (node === null) throw new Error(`missing ${selector}`)
  return node
}

function accessibleName(control: HTMLElement): string {
  return control.getAttribute('aria-label')?.trim() || control.textContent?.trim() || ''
}

function isVisible(node: HTMLElement): boolean {
  const style = getComputedStyle(node)
  const bounds = node.getBoundingClientRect()
  return !node.hidden && style.display !== 'none' && bounds.width > 0 && bounds.height > 0
}
