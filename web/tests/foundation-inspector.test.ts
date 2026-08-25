import { afterEach, describe, expect, it, vi } from 'vitest'

import {
  IndexedDbDocumentStore,
  type PlannedPersistenceCommitDto,
} from '../persistence/indexeddb-store'
import * as inspectorModule from '../src/foundation-inspector'
import {
  mountFoundationInspector,
  type FoundationInspectorController,
} from '../src/foundation-inspector'
// Vite supplies raw HTML to the real-browser test; the production TS config has no asset modules.
// @ts-expect-error -- `?raw` is resolved by Vite at test runtime.
import inspectorIndexHtml from '../index.html?raw'

type Locale = 'uk' | 'en'

interface MountOptionsContract {
  readonly databaseName: string
  readonly locale?: Locale
  readonly clock?: () => Date
}

interface LocalizationContract {
  readonly foundationInspectorMessages: Readonly<
    Record<Locale, Readonly<Record<string, string>>>
  >
}

const mountWithOptions = mountFoundationInspector as unknown as (
  root: HTMLElement,
  options: MountOptionsContract,
) => Promise<FoundationInspectorController>

const localization = inspectorModule as unknown as LocalizationContract

function rootFixture(): HTMLElement {
  const root = document.createElement('div')
  document.body.replaceChildren(root)
  return root
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

afterEach(() => {
  vi.restoreAllMocks()
})

describe('Foundation Inspector approved contract', () => {
  it('keeps Ukrainian and English resources key-identical and renders English by locale', async () => {
    const ukKeys = Object.keys(localization.foundationInspectorMessages.uk).sort()
    const enKeys = Object.keys(localization.foundationInspectorMessages.en).sort()
    expect(ukKeys).toEqual(enKeys)
    expect(ukKeys).toContain('foundationInspector.provenance.noExport')
    expect(ukKeys).toEqual(
      expect.arrayContaining([
        'foundationInspector.documentTitle',
        'foundationInspector.noscript',
        'foundationInspector.error.startup',
      ]),
    )
    expect(
      localization.foundationInspectorMessages.en['foundationInspector.documentTitle'],
    ).toBe('FlowPDF — Foundation Inspector')
    expect(
      localization.foundationInspectorMessages.uk[
        'foundationInspector.copy.documentId.noun'
      ],
    ).toBe('ID документа')
    expect(
      localization.foundationInspectorMessages.en['foundationInspector.copy.hash.noun'],
    ).toBe('Revision hash')

    const root = rootFixture()
    const inspector = await mountWithOptions(root, {
      databaseName: 'flowpdf-inspector-locale-unit',
      locale: 'en',
    })

    expect(root.lang).toBe('en')
    expect(action(root, 'create-sample').textContent).toBe('Create sample document')
    expect(root.querySelector('aside')?.getAttribute('aria-label')).toBe('Document inspector')
    expect(root.textContent).toContain(
      'Preview or export provenance will be available in the next phase.',
    )
    await clickAndWait(inspector, root, 'create-sample')
    expect(inspector.snapshot().locale).toBe('en-US')
  })

  it('keeps the no-JavaScript title synchronized and overrides it for English runtime', async () => {
    const fallbackDocument = new DOMParser().parseFromString(
      inspectorIndexHtml,
      'text/html',
    )
    expect(fallbackDocument.title).toBe(
      localization.foundationInspectorMessages.uk['foundationInspector.documentTitle'],
    )
    expect(fallbackDocument.querySelector('noscript')?.textContent?.trim()).toBe(
      localization.foundationInspectorMessages.uk['foundationInspector.noscript'],
    )

    const frame = document.createElement('iframe')
    frame.srcdoc = `<!doctype html><html lang="en"><head><title>${fallbackDocument.title}</title></head><body><div id="app"></div><script type="module" src="${new URL('../src/main.ts', import.meta.url).href}"></script></body></html>`
    document.body.replaceChildren(frame)
    try {
      await waitUntil(
        () =>
          frame.contentDocument?.title ===
          localization.foundationInspectorMessages.en[
            'foundationInspector.documentTitle'
          ],
      )
      expect(frame.contentDocument?.title).toBe('FlowPDF — Foundation Inspector')
    } finally {
      frame.remove()
    }
  })

  it('renders semantic empty, populated, audit, copy, and partial-provenance states', async () => {
    const root = rootFixture()
    const inspector = await mountWithOptions(root, {
      databaseName: 'flowpdf-inspector-states-unit',
      locale: 'uk',
    })

    expect(root.querySelectorAll('header')).toHaveLength(1)
    expect(root.querySelectorAll('main')).toHaveLength(1)
    expect(root.querySelector('section[aria-labelledby="commands-heading"]')).not.toBeNull()
    expect(root.querySelector('aside[aria-label="Інспектор документа"]')).not.toBeNull()
    expect(root.querySelector<HTMLElement>('[data-audit-empty]')?.hidden).toBe(false)
    expect(root.querySelector('[data-audit-count]')?.textContent).toContain('0')
    expect(root.querySelector<HTMLElement>('.audit-table-wrapper')?.hidden).toBe(true)

    await clickAndWait(inspector, root, 'create-sample')
    expect(inspector.snapshot().locale).toBe('uk-UA')

    const auditTable = root.querySelector<HTMLTableElement>('table[data-audit]')
    expect(auditTable).not.toBeNull()
    expect(root.querySelector<HTMLElement>('.audit-table-wrapper')?.hidden).toBe(false)
    expect(auditTable?.querySelectorAll('th[scope="col"]')).toHaveLength(6)
    expect(auditTable?.querySelectorAll('tbody tr[data-audit-row]')).toHaveLength(1)
    expect(auditTable?.querySelectorAll('tbody td[data-label][headers]')).toHaveLength(6)
    expect(
      auditTable?.querySelectorAll('tbody .audit-cell-label[aria-hidden="true"]'),
    ).toHaveLength(6)
    expect(auditTable?.querySelector('time[datetime]')).not.toBeNull()
    expect(root.querySelector('[data-audit-count]')?.textContent).toContain('1')

    const snapshot = inspector.snapshot()
    const idCopy = root.querySelector<HTMLButtonElement>('[data-copy="document-id"]')
    const hashCopy = root.querySelector<HTMLButtonElement>('[data-copy="revision-hash"]')
    expect(idCopy?.getAttribute('aria-label')).toContain(snapshot.documentId)
    expect(hashCopy?.getAttribute('aria-label')).toContain(snapshot.hash)
    expect(root.querySelector('[data-full-document-id]')?.textContent).toBe(snapshot.documentId)
    expect(root.querySelector('[data-full-revision-hash]')?.textContent).toBe(snapshot.hash)
    expect(root.textContent).toContain('—')

    expect(root.textContent).not.toMatch(
      /Український|English sample|typed mutation|rawTranscript|rawAudio|commandArguments|canonicalJson/i,
    )
  })

  it('keeps stale failure atomic and gives buttons and keyboard identical history behavior', async () => {
    const root = rootFixture()
    const inspector = await mountWithOptions(root, {
      databaseName: 'flowpdf-inspector-history-unit',
      locale: 'uk',
    })
    await clickAndWait(inspector, root, 'create-sample')
    expect(action(root, 'undo').disabled).toBe(true)
    await clickAndWait(inspector, root, 'apply-mutation')
    await clickAndWait(inspector, root, 'apply-mutation')
    const applied = inspector.snapshot()

    await clickAndWait(inspector, root, 'stale-command')
    const rejected = inspector.snapshot()
    expect(rejected.revision).toBe(applied.revision)
    expect(rejected.hash).toBe(applied.hash)
    expect(rejected.audit).toHaveLength(applied.audit.length + 1)
    expect(rejected.audit.at(-1)).toMatchObject({
      documentId: applied.documentId,
      baseRevision: applied.revision,
      newRevision: applied.revision,
      action: { type: 'command', commandKind: 'insertText' },
      outcome: { kind: 'failure', code: 'staleRevision' },
    })
    expect(JSON.stringify(rejected.audit.at(-1))).not.toMatch(
      /stale diagnostic|typed mutation|canonicalJson|commandArguments/i,
    )
    const conflictCopy = root.querySelector('.alert-region[role="alert"]')?.textContent
    expect(conflictCopy).toContain('FLOW_STALE_REVISION')
    expect(conflictCopy).toMatch(/не змінено/i)

    const undo = action(root, 'undo')
    undo.focus()
    const shortcut = new KeyboardEvent('keydown', {
      key: 'z',
      ctrlKey: true,
      bubbles: true,
      cancelable: true,
    })
    root.dispatchEvent(shortcut)
    await inspector.whenIdle()
    expect(shortcut.defaultPrevented).toBe(true)
    expect(inspector.snapshot().revision).toBe(4)
    expect(document.activeElement).toBe(undo)
    expect(action(root, 'redo').disabled).toBe(false)

    const input = document.createElement('input')
    root.append(input)
    input.focus()
    const inputShortcut = new KeyboardEvent('keydown', {
      key: 'z',
      ctrlKey: true,
      bubbles: true,
      cancelable: true,
    })
    input.dispatchEvent(inputShortcut)
    await inspector.whenIdle()
    expect(inputShortcut.defaultPrevented).toBe(false)
    expect(inspector.snapshot().revision).toBe(4)
  })

  it('retains the prior view and disables every conflicting action while work is pending', async () => {
    let releaseCommit = (): void => undefined
    const commitGate = new Promise<void>((resolve) => {
      releaseCommit = resolve
    })
    const realCommit = IndexedDbDocumentStore.prototype.commit
    vi.spyOn(IndexedDbDocumentStore.prototype, 'commit').mockImplementation(async function commit(
      this: IndexedDbDocumentStore,
      planned: PlannedPersistenceCommitDto,
    ) {
      await commitGate
      await realCommit.call(this, planned)
    })

    const root = rootFixture()
    const inspector = await mountWithOptions(root, {
      databaseName: 'flowpdf-inspector-pending-unit',
      locale: 'uk',
    })
    const create = action(root, 'create-sample')
    const openOlder = action(root, 'open-older-schema')
    create.click()
    await waitUntil(() => root.getAttribute('aria-busy') === 'true')

    try {
      expect(
        [...root.querySelectorAll<HTMLButtonElement>('button[data-action]')].every(
          ({ disabled }) => disabled,
        ),
      ).toBe(true)
      expect(root.querySelector<HTMLElement>('[data-empty-document]')?.hidden).toBe(false)
      expect(root.querySelector('[data-durability-status]')?.textContent).toContain(
        'Виконується',
      )
    } finally {
      releaseCommit()
      await inspector.whenIdle()
    }
    expect(create.hidden).toBe(true)
    expect(openOlder.hidden).toBe(true)
    expect(action(root, 'apply-mutation').disabled).toBe(false)
  })

  it('keeps delayed and out-of-order copy feedback separate from operation lifecycle', async () => {
    const root = rootFixture()
    const inspector = await mountWithOptions(root, {
      databaseName: 'flowpdf-inspector-copy-during-pending-unit',
      locale: 'uk',
    })
    await clickAndWait(inspector, root, 'create-sample')
    const before = inspector.snapshot()

    let releaseCommit = (): void => undefined
    const commitGate = new Promise<void>((resolve) => {
      releaseCommit = resolve
    })
    const realCommit = IndexedDbDocumentStore.prototype.commit
    vi.spyOn(IndexedDbDocumentStore.prototype, 'commit').mockImplementation(async function commit(
      this: IndexedDbDocumentStore,
      planned: PlannedPersistenceCommitDto,
    ) {
      await commitGate
      await realCommit.call(this, planned)
    })
    let releaseClipboard = (): void => undefined
    const clipboardGate = new Promise<void>((resolve) => {
      releaseClipboard = resolve
    })
    let releaseStaleClipboard = (): void => undefined
    const staleClipboardGate = new Promise<void>((resolve) => {
      releaseStaleClipboard = resolve
    })
    let rejectClipboard = (): void => undefined
    const clipboardFailureGate = new Promise<void>((_resolve, reject) => {
      rejectClipboard = () => reject(new Error('clipboard denied'))
    })
    const clipboardWrite = vi
      .spyOn(navigator.clipboard, 'writeText')
      .mockReturnValue(clipboardGate)

    action(root, 'apply-mutation').click()
    await waitUntil(() => root.getAttribute('aria-busy') === 'true')
    const lifecycleStatus = root.querySelector<HTMLElement>('[data-durability-status]')
    const copyStatus = root.querySelector<HTMLElement>('[data-copy-status]')
    const copyAlert = root.querySelector<HTMLElement>('[data-copy-alert]')

    try {
      const enabledControls = [
        ...root.querySelectorAll<HTMLButtonElement>('button:not([disabled])'),
      ].filter(({ hidden }) => !hidden)
      expect(enabledControls.map(({ dataset }) => dataset.copy)).toEqual([
        'document-id',
        'revision-hash',
      ])

      const copyDocumentId = enabledControls[0]
      copyDocumentId?.click()
      await waitUntil(() => clipboardWrite.mock.calls.length === 1)
      expect(clipboardWrite).toHaveBeenCalledWith(before.documentId)
      expect(lifecycleStatus?.textContent).toContain('Виконується')
      expect(copyStatus?.textContent).toBe('')

      releaseCommit()
      await inspector.whenIdle()
      const operationCompletion = lifecycleStatus?.textContent
      expect(operationCompletion).toContain('Команду виконано')

      releaseClipboard()
      await waitUntil(() => copyStatus?.textContent === 'Скопійовано ID документа.')
      expect(lifecycleStatus?.textContent).toBe(operationCompletion)
      expect(copyStatus?.getAttribute('role')).toBe('status')
      expect(copyStatus?.getAttribute('aria-live')).toBe('polite')

      clipboardWrite
        .mockReturnValueOnce(staleClipboardGate)
        .mockReturnValueOnce(clipboardFailureGate)
      root.querySelector<HTMLButtonElement>('[data-copy="document-id"]')?.click()
      root.querySelector<HTMLButtonElement>('[data-copy="revision-hash"]')?.click()
      await waitUntil(() => clipboardWrite.mock.calls.length === 3)
      expect(copyStatus?.textContent).toBe('')
      expect(copyAlert?.textContent).toBe('')
      rejectClipboard()
      await waitUntil(
        () => copyAlert?.textContent.includes('FLOW_CLIPBOARD_WRITE_FAILED') === true,
      )
      expect(copyAlert?.getAttribute('role')).toBe('alert')
      expect(root.querySelector<HTMLElement>('.alert-region')?.hidden).toBe(true)
      expect(lifecycleStatus?.textContent).toBe(operationCompletion)
      const failureFeedback = copyAlert?.textContent

      releaseStaleClipboard()
      await staleClipboardGate
      await Promise.resolve()
      expect(copyStatus?.textContent).toBe('')
      expect(copyAlert?.textContent).toBe(failureFeedback)
    } finally {
      releaseCommit()
      releaseClipboard()
      releaseStaleClipboard()
      if (clipboardWrite.mock.calls.length >= 3) rejectClipboard()
      await inspector.whenIdle()
      if (clipboardWrite.mock.calls.length === 1) {
        await waitUntil(() => copyStatus?.textContent !== '')
      } else if (clipboardWrite.mock.calls.length === 2) {
        await waitUntil(() => copyStatus?.textContent !== '')
      } else if (clipboardWrite.mock.calls.length >= 3) {
        await waitUntil(() => copyAlert?.textContent !== '')
      }
    }
  })

  it('keeps adjacent equal-time audit identities distinct and in durable source order', async () => {
    const root = rootFixture()
    const inspector = await mountWithOptions(root, {
      databaseName: 'flowpdf-inspector-audit-unit',
      locale: 'uk',
      clock: () => new Date('2026-08-14T00:00:01.987Z'),
    })
    await clickAndWait(inspector, root, 'create-sample')
    await clickAndWait(inspector, root, 'apply-mutation')
    await clickAndWait(inspector, root, 'undo')
    await clickAndWait(inspector, root, 'redo')

    const rows = [...root.querySelectorAll<HTMLElement>('[data-audit-row]')]
    expect(rows).toHaveLength(4)
    expect(rows.map(({ dataset }) => dataset.auditId)).toHaveLength(4)
    expect(new Set(rows.map(({ dataset }) => dataset.auditId)).size).toBe(4)
    expect(rows.map(({ dataset }) => Number(dataset.revision))).toEqual([1, 2, 3, 4])
    expect(rows.slice(1).map((row) => row.querySelector('time')?.dateTime)).toEqual([
      '2026-08-14T00:00:01Z',
      '2026-08-14T00:00:01Z',
      '2026-08-14T00:00:01Z',
    ])
  })

  it('stamps each browser command from the injected clock', async () => {
    const instants = [
      '2026-08-14T12:00:01.123Z',
      '2026-08-14T12:00:02.456Z',
      '2026-08-14T12:00:03.789Z',
      '2026-08-14T12:00:04.987Z',
    ]
    let clockIndex = 0
    const root = rootFixture()
    const inspector = await mountWithOptions(root, {
      databaseName: 'flowpdf-inspector-clock-unit',
      locale: 'uk',
      clock: () => new Date(instants[clockIndex++] ?? '2026-08-14T12:00:04.987Z'),
    })

    await clickAndWait(inspector, root, 'create-sample')
    await clickAndWait(inspector, root, 'apply-mutation')
    await clickAndWait(inspector, root, 'undo')
    await clickAndWait(inspector, root, 'redo')

    const timestamps = [...root.querySelectorAll<HTMLTimeElement>('[data-audit-row] time')]
      .map(({ dateTime }) => dateTime)
    expect(timestamps).toEqual([
      '2026-08-14T12:00:01Z',
      '2026-08-14T12:00:02Z',
      '2026-08-14T12:00:03Z',
      '2026-08-14T12:00:04Z',
    ])
  })
})

async function waitUntil(predicate: () => boolean): Promise<void> {
  for (let attempt = 0; attempt < 20; attempt += 1) {
    if (predicate()) return
    await new Promise((resolve) => setTimeout(resolve, 0))
  }
  throw new Error('condition was not reached')
}
