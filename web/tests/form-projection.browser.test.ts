import { page } from 'vitest/browser'
import { expect, test } from 'vitest'
import { createElement } from 'react'
import { createRoot, type Root } from 'react-dom/client'

import '../src/styles.css'
import { EditorApp, EditorController, type EditorLayoutScheduler } from '../src/editor/editor-app.js'
import type {
  AcceptedLayoutDto,
  LayoutRequestDto,
  LayoutSchedulerSnapshotDto,
  LayoutWorkerResultDto,
} from '../src/layout/layout-protocol.js'
import type { LayoutScheduleOutcome } from '../src/layout/layout-worker.js'
import type {
  AcceptedFormProjectionDto,
  FormProjectionRequestDto,
  FormProjectionResultDto,
  FormProjectionScheduleOutcome,
  FormProjectionScheduler,
  FormProjectionSchedulerSnapshotDto,
} from '../src/forms/form-projection.js'

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
    this.state = { phase: 'ready', accepted, requestId: null, errorCode: null }
    this.emit()
    entry.resolve({ kind: 'published', accepted })
  }

  private emit(): void {
    for (const listener of this.listeners) listener()
  }
}

class BrowserFormProjectionScheduler implements FormProjectionScheduler {
  private readonly listeners = new Set<() => void>()
  private state: FormProjectionSchedulerSnapshotDto = {
    phase: 'idle',
    accepted: null,
    requestId: null,
    errorCode: null,
  }
  private readonly pending = new Map<
    string,
    { readonly request: FormProjectionRequestDto; readonly resolve: (outcome: FormProjectionScheduleOutcome) => void }
  >()
  readonly requests: FormProjectionRequestDto[] = []

  request(request: FormProjectionRequestDto): Promise<FormProjectionScheduleOutcome> {
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

  accepted(): AcceptedFormProjectionDto | null {
    return this.state.accepted
  }

  snapshot(): FormProjectionSchedulerSnapshotDto {
    return this.state
  }

  subscribe(listener: () => void): () => void {
    this.listeners.add(listener)
    return () => this.listeners.delete(listener)
  }

  publish(request: FormProjectionRequestDto, result: FormProjectionResultDto): void {
    if (this.state.requestId !== request.requestId) return
    const entry = this.pending.get(request.requestId)
    if (entry === undefined) return
    this.pending.delete(request.requestId)
    const accepted = Object.freeze({ request, result })
    this.state = { phase: 'ready', accepted, requestId: null, errorCode: null }
    this.emit()
    entry.resolve({ kind: 'published', accepted })
  }

  private emit(): void {
    for (const listener of this.listeners) listener()
  }
}

test('editor renders current Rust form widgets visually and reports projection review accessibly', async () => {
  await page.viewport(1280, 1000)
  const root = document.createElement('div')
  document.body.replaceChildren(root)
  const layout = new BrowserLayoutScheduler()
  const projection = new BrowserFormProjectionScheduler()
  const controller = new EditorController(
    {
      databaseName: `flowpdf-form-projection-${crypto.randomUUID()}`,
      locale: 'uk',
      clock: () => new Date('2026-09-22T00:00:00Z'),
    },
    {
      layoutScheduler: layout,
      layoutRequestFactory: (accepted) => ({
        protocolVersion: 1,
        requestId: `browser-layout-${accepted.session.revision}`,
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
  reactRoot.render(
    createElement(EditorApp, {
      controller,
      formProjectionScheduler: projection,
      options: { locale: 'uk' },
    }),
  )
  await controller.initialize()
  await tick()
  root.querySelector<HTMLButtonElement>('[data-action="editor-open-older"]')?.click()
  await settleForm(root, controller)
  await settle(controller)

  const layoutRequest = layout.requests[0]
  if (layoutRequest === undefined) throw new Error('layout request is required')
  layout.publish(layoutRequest, layoutResult(layoutRequest, 'layout-one'))
  await waitFor(() => projection.requests.length > 0)
  const projectionRequest = projection.requests.at(-1)
  if (projectionRequest === undefined) throw new Error('projection request is required')

  projection.publish(projectionRequest, projectionResult(projectionRequest, false))
  await tick()
  expect(root.querySelector('[data-form-projection-status]')?.textContent).toContain('синхронізовано')
  expect(root.querySelectorAll('[data-form-projection-widget]')).toHaveLength(1)
  expect(root.querySelectorAll('[data-form-widget-overlay]')).toHaveLength(1)
  expect(root.querySelector('[data-form-widget-overlays]')?.getAttribute('aria-hidden')).toBe('true')
  expect(root.querySelector('[data-form-projection-review]')).toBeNull()

  const reviewRequest = { ...projectionRequest, requestId: `${projectionRequest.requestId}-review` }
  void projection.request(reviewRequest)
  await tick()
  projection.publish(reviewRequest, projectionResult(reviewRequest, true))
  await tick()
  expect(root.querySelector('[data-form-projection-review]')).not.toBeNull()
  expect(root.querySelector('[data-form-projection-review-entry]')?.textContent).toContain('field-1')
  expect(root.querySelectorAll('[data-form-widget-overlay]')).toHaveLength(0)
  expect(root.querySelector('[data-editor-input-host]')).not.toBeNull()

  reactRoot.unmount()
  controller.dispose()
})

test('source change removes stale form geometry until a matching layout/projection arrives', async () => {
  const root = document.createElement('div')
  document.body.replaceChildren(root)
  const layout = new BrowserLayoutScheduler()
  const projection = new BrowserFormProjectionScheduler()
  const controller = new EditorController(
    {
      databaseName: `flowpdf-form-projection-stale-${crypto.randomUUID()}`,
      locale: 'en',
      clock: () => new Date('2026-09-22T00:00:00Z'),
    },
    {
      layoutScheduler: layout,
      layoutRequestFactory: (accepted) => ({
        protocolVersion: 1,
        requestId: `browser-layout-${accepted.session.revision}`,
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
  reactRoot.render(
    createElement(EditorApp, {
      controller,
      formProjectionScheduler: projection,
      options: { locale: 'en' },
    }),
  )
  await controller.initialize()
  await tick()
  root.querySelector<HTMLButtonElement>('[data-action="editor-open-older"]')?.click()
  await settleForm(root, controller)
  await settle(controller)
  const firstLayout = layout.requests[0]
  if (firstLayout === undefined) throw new Error('first layout request is required')
  layout.publish(firstLayout, layoutResult(firstLayout, 'layout-one'))
  await waitFor(() => projection.requests.length > 0)
  const firstProjection = projection.requests.at(-1)
  if (firstProjection === undefined) throw new Error('first projection request is required')
  projection.publish(firstProjection, projectionResult(firstProjection, false))
  await tick()
  expect(root.querySelectorAll('[data-form-widget-overlay]')).toHaveLength(1)

  await controller.replaceSelection()
  await settle(controller)
  expect(root.querySelectorAll('[data-form-widget-overlay]')).toHaveLength(0)
  expect(root.querySelector('[data-form-projection-status]')?.textContent).toContain('Waiting')
  expect(root.querySelector('[data-editor-input-host]')).not.toBeNull()

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
    pages: [
      {
        pageIndex: 0,
        sectionId: '00000000-0000-4000-8000-000000000701',
        pageSettings: {},
        bounds: { x: 0, y: 0, width: 4_800, height: 6_400 },
        contentRect: { x: 320, y: 320, width: 4_160, height: 5_760 },
        startReason: 'documentStart',
        header: null,
        footer: null,
        fragments: [],
      },
    ],
    diagnostics: [],
    resultHash,
  }
}

function projectionResult(
  request: FormProjectionRequestDto,
  review: boolean,
): FormProjectionResultDto {
  return {
    protocolVersion: 1,
    requestId: request.requestId,
    sourceRevision: request.sourceRevision,
    sourceHash: request.sourceHash,
    layoutResultHash: request.layoutResultHash,
    displayListHash: `display-${request.sourceRevision}`,
    projection: {
      schemaVersion: 2,
      sourceRevision: request.sourceRevision,
      sourceHash: request.sourceHash,
      displayListHash: `display-${request.sourceRevision}`,
      widgets: review
        ? []
        : [
            {
              widgetId: 'widget-1',
              fieldId: 'field-1',
              name: 'name',
              label: 'Name',
              kind: { type: 'text', multiline: false, inputHint: 'plain' },
              required: false,
              readOnly: false,
              defaultValue: { type: 'text', value: 'Тест' },
              value: { type: 'text', value: 'Тест' },
              pageIndex: 0,
              rect: { x: 320, y: 480, width: 800, height: 64 },
              sourceNodeId: 'node-1',
              anchorOffsetUtf16: 4,
              tabOrder: 0,
            },
          ],
      review: review
        ? [
            {
              fieldId: 'field-1',
              sourceNodeId: 'node-missing',
              reason: { kind: 'targetMissing' },
            },
          ]
        : [],
      resultHash: `projection-${request.sourceRevision}-${review ? 'review' : 'ready'}`,
    },
    formPlan: null,
  }
}

async function settle(controller: EditorController): Promise<void> {
  await controller.whenIdle()
  await tick()
}

async function settleForm(root: HTMLElement, controller: EditorController): Promise<void> {
  for (let attempt = 0; attempt < 40; attempt += 1) {
    await settle(controller)
    if (root.querySelector('[data-form-session-status="loading"]') === null) return
  }
  throw new Error('form session did not settle')
}

async function waitFor(predicate: () => boolean): Promise<void> {
  for (let attempt = 0; attempt < 40; attempt += 1) {
    if (predicate()) return
    await tick()
  }
  throw new Error('projection request did not arrive')
}

async function tick(): Promise<void> {
  await new Promise<void>((resolve) => setTimeout(resolve, 0))
}
