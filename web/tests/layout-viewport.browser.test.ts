import { page } from 'vitest/browser'
import { expect, test } from 'vitest'
import { createElement } from 'react'
import { createRoot, type Root } from 'react-dom/client'

import '../src/styles.css'
import {
  EditorApp,
  EditorController,
  type EditorLayoutScheduler,
} from '../src/editor/editor-app.js'
import type {
  AcceptedLayoutDto,
  LayoutRequestDto,
  LayoutSchedulerSnapshotDto,
  LayoutWorkerResultDto,
} from '../src/layout/layout-protocol.js'
import type { LayoutScheduleOutcome } from '../src/layout/layout-worker.js'

class BrowserLayoutScheduler implements EditorLayoutScheduler {
  private readonly listeners = new Set<() => void>()
  private readonly pending = new Map<
    string,
    { readonly request: LayoutRequestDto; readonly resolve: (outcome: LayoutScheduleOutcome) => void }
  >()
  private state: LayoutSchedulerSnapshotDto = {
    phase: 'idle',
    accepted: null,
    requestId: null,
    errorCode: null,
  }
  readonly requests: LayoutRequestDto[] = []

  request(request: LayoutRequestDto): Promise<LayoutScheduleOutcome> {
    this.requests.push(request)
    this.state = {
      phase: 'pending',
      accepted: this.state.accepted,
      requestId: request.requestId,
      errorCode: null,
    }
    this.emit()
    return new Promise((resolve) => {
      this.pending.set(request.requestId, { request, resolve })
    })
  }

  cancel(requestId?: string): void {
    for (const [id, entry] of this.pending) {
      if (requestId !== undefined && requestId !== id) continue
      this.pending.delete(id)
      entry.resolve({ kind: 'cancelled', requestId: id })
    }
    this.state = {
      phase: this.state.accepted === null ? 'idle' : 'ready',
      accepted: this.state.accepted,
      requestId: null,
      errorCode: null,
    }
    this.emit()
  }

  accepted(): AcceptedLayoutDto | null {
    return this.state.accepted
  }

  snapshot(): LayoutSchedulerSnapshotDto {
    return this.state
  }

  subscribe(listener: () => void): () => void {
    this.listeners.add(listener)
    return () => this.listeners.delete(listener)
  }

  publish(request: LayoutRequestDto, result: LayoutWorkerResultDto): void {
    if (this.state.requestId !== request.requestId) return
    const entry = this.pending.get(request.requestId)
    if (entry === undefined) return
    this.pending.delete(request.requestId)
    const accepted = Object.freeze({ request, result })
    this.state = {
      phase: 'ready',
      accepted,
      requestId: null,
      errorCode: null,
    }
    this.emit()
    entry.resolve({ kind: 'published', accepted })
  }

  discard(request: LayoutRequestDto, code: string): void {
    if (this.state.requestId !== request.requestId) return
    const entry = this.pending.get(request.requestId)
    if (entry === undefined) return
    this.pending.delete(request.requestId)
    this.state = {
      phase: 'error',
      accepted: this.state.accepted,
      requestId: null,
      errorCode: code,
    }
    this.emit()
    entry.resolve({ kind: 'discarded', requestId: request.requestId, code })
  }

  private emit(): void {
    for (const listener of this.listeners) listener()
  }
}

test('viewport remains a visual Rust projection while semantic editing stays available', async () => {
  await page.viewport(1280, 1000)
  const root = document.createElement('div')
  document.body.replaceChildren(root)
  const scheduler = new BrowserLayoutScheduler()
  let requestNumber = 0
  const controller = new EditorController(
    {
      databaseName: `flowpdf-layout-viewport-${crypto.randomUUID()}`,
      locale: 'uk',
      clock: () => new Date('2026-09-21T00:00:00Z'),
    },
    {
      layoutScheduler: scheduler,
      layoutRequestFactory: (accepted) => ({
        protocolVersion: 1,
        requestId: `browser-layout-${++requestNumber}`,
        sourceRevision: accepted.session.revision,
        sourceHash: accepted.session.canonicalHash,
        expectedLayoutSettingsFingerprint: null,
        fontCatalogIdentity: 'browser-fixture-fonts',
        hyphenationDataIdentity: null,
        viewport: { firstPage: 0, pageCount: 4 },
        serializedRequest: accepted.session.canonicalJson,
      }),
    },
  )
  const reactRoot: Root = createRoot(root)
  reactRoot.render(createElement(EditorApp, { controller, options: { locale: 'uk' } }))
  await controller.initialize()
  await tick()

  root.querySelector<HTMLButtonElement>('[data-action="editor-open-older"]')?.click()
  await settle(controller)
  const firstRequest = scheduler.requests[0]
  if (firstRequest === undefined) throw new Error('first layout request is required')
  expect(root.querySelector('[data-editor-document]')).not.toBeNull()
  expect(root.querySelector('[data-layout-status]')?.textContent).toContain('фоні')
  expect(root.querySelectorAll('[data-layout-page]')).toHaveLength(0)

  scheduler.publish(firstRequest, layoutResult(firstRequest, 'result-one'))
  await tick()
  expect(root.querySelectorAll('[data-editor-document]')).toHaveLength(1)
  expect(root.querySelector('[data-semantic-source-revision]')?.getAttribute('data-semantic-source-revision')).toBe(
    String(firstRequest.sourceRevision),
  )
  expect(root.querySelector('[data-layout-viewport]')?.getAttribute('data-layout-source-revision')).toBe(
    String(firstRequest.sourceRevision),
  )
  expect(root.querySelector('[data-layout-result-hash]')?.getAttribute('data-layout-result-hash')).toBe(
    'result-one',
  )
  expect(root.querySelectorAll('[data-layout-page]')).toHaveLength(2)
  expect(root.querySelector('[data-layout-page]')?.getAttribute('aria-hidden')).toBe('true')
  expect(root.querySelector('[data-layout-fragment]')?.getAttribute('data-source-node-id')).toBe(
    '00000000-0000-4000-8000-000000000901',
  )

  const semanticInput = root.querySelector<HTMLTextAreaElement>('[data-editor-input-host]')
  semanticInput?.focus()
  expect(document.activeElement).toBe(semanticInput)
  await controller.replaceSelection()
  await settle(controller)
  const secondRequest = scheduler.requests[1]
  if (secondRequest === undefined) throw new Error('second layout request is required')
  expect(root.querySelector('[data-semantic-source-revision]')?.getAttribute('data-semantic-source-revision')).toBe(
    String(secondRequest.sourceRevision),
  )
  expect(root.querySelectorAll('[data-layout-page]')).toHaveLength(0)
  expect(root.querySelector('[data-layout-status]')?.textContent).toContain('фоні')
  expect(root.querySelector('[data-editor-input-host]')).not.toBeNull()

  // A late result from the previous accepted revision is ignored by the
  // scheduler fixture and cannot repopulate a stale visual page tree.
  scheduler.publish(firstRequest, layoutResult(firstRequest, 'late-old-result'))
  await tick()
  expect(root.querySelectorAll('[data-layout-page]')).toHaveLength(0)

  scheduler.publish(secondRequest, layoutResult(secondRequest, 'result-two'))
  await tick()
  expect(root.querySelectorAll('[data-layout-page]')).toHaveLength(2)
  expect(root.querySelector('[data-layout-result-hash]')?.getAttribute('data-layout-result-hash')).toBe(
    'result-two',
  )
  reactRoot.unmount()
  controller.dispose()
})

function layoutResult(request: LayoutRequestDto, resultHash: string): LayoutWorkerResultDto {
  return {
    protocolVersion: 1,
    requestId: request.requestId,
    sourceRevision: request.sourceRevision,
    sourceHash: request.sourceHash,
    layoutSettingsFingerprint: 'browser-settings',
    fontCatalogIdentity: request.fontCatalogIdentity,
    hyphenationDataIdentity: request.hyphenationDataIdentity,
    pages: [0, 1].map((pageIndex) => ({
      pageIndex,
      sectionId: '00000000-0000-4000-8000-000000000701',
      pageSettings: {},
      bounds: { x: 0, y: 0, width: 4_800, height: 6_400 },
      contentRect: { x: 320, y: 320, width: 4_160, height: 5_760 },
      startReason: pageIndex === 0 ? 'documentStart' : 'natural',
      header: null,
      footer: null,
      fragments: [
        {
          id: `fragment-${request.sourceRevision}-${pageIndex}`,
          kind: 'paragraph',
          sourceNodeId: '00000000-0000-4000-8000-000000000901',
          source: {
            utf8Start: 0,
            utf8End: 10,
            utf16Start: 0,
            utf16End: 10,
          },
          rect: { x: 0, y: 0, width: 3_000, height: 200 },
          breakReason: null,
          derived: false,
          repeatIndex: 0,
          children: [],
        },
      ],
    })),
    diagnostics: [],
    resultHash,
  }
}

async function settle(controller: EditorController): Promise<void> {
  await controller.whenIdle()
  await tick()
}

async function tick(): Promise<void> {
  await new Promise<void>((resolve) => setTimeout(resolve, 0))
}
