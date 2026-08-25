import { page } from 'vitest/browser'
import { expect, test } from 'vitest'

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
      expect(root.querySelector('[role="alert"]')).not.toBeNull()
      expect(root.querySelector('[data-provenance-unavailable]')?.textContent).toMatch(
        copy.unavailableProvenance,
      )

      const create = action(root, 'create-sample')
      create.focus()
      const focusStyle = getComputedStyle(create)
      expect(focusStyle.outlineStyle).toBe('solid')
      expect(focusStyle.outlineWidth).toBe('3px')
      expect(focusStyle.outlineOffset).toBe('2px')
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

      const fullDocumentId = root.querySelector<HTMLElement>('[data-full-document-id]')
      const fullHash = root.querySelector<HTMLElement>('[data-full-revision-hash]')
      const hashCopy = root.querySelector<HTMLButtonElement>('[data-copy="revision-hash"]')
      expect(fullDocumentId?.textContent).toBe(created.documentId)
      expect(fullHash?.textContent).toBe(created.hash)
      expect(fullHash?.getAttribute('aria-label')).toBe(created.hash)
      expect(hashCopy?.getAttribute('aria-label')).toContain(created.hash)

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
      expect(root.querySelector('[role="alert"]')?.textContent).toContain(
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
