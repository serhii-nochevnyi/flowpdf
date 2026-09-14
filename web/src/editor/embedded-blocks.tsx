import { useEffect, useRef, useState } from 'react'

import type {
  ConfirmationMetadataDto,
  EditorBlockViewDto,
  EditorLocale,
  EditorViewDto,
  ImageAccessibilityDto,
  StructuralPlacementDto,
} from './editor-store.js'
import type {
  StructuralCommandTarget,
} from './editor-controller.js'
import { editorMessages } from './editor-messages.js'

type AuthoredImageAccessibility = Exclude<
  ImageAccessibilityDto,
  { readonly kind: 'missingLegacy' }
>

interface ImageCommandTarget extends StructuralCommandTarget {}

export interface ImageInsertDialogProps {
  readonly controller: ImageCommandTarget
  readonly locale: EditorLocale
  readonly placement?: StructuralPlacementDto
  readonly imageNodeId?: string
  readonly disabled: boolean
}

export interface ImageActionBarProps {
  readonly block: EditorBlockViewDto
  readonly view: EditorViewDto
  readonly controller: ImageCommandTarget
  readonly locale: EditorLocale
  readonly onRequestRemoveImage: (
    imageNodeId: string,
    confirmation: ConfirmationMetadataDto,
  ) => void
}

export function ImageInsertDialog({
  controller,
  locale,
  placement,
  imageNodeId,
  disabled,
}: ImageInsertDialogProps) {
  const labels = editorMessages[locale]
  const replace = imageNodeId !== undefined
  const [open, setOpen] = useState(false)
  const [file, setFile] = useState<File | null>(null)
  const [description, setDescription] = useState('')
  const [decorative, setDecorative] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [previewUrl, setPreviewUrl] = useState<string | null>(null)
  const cancelRef = useRef<HTMLButtonElement>(null)
  const fileRef = useRef<HTMLInputElement>(null)

  useEffect(() => {
    if (!open) return
    cancelRef.current?.focus()
  }, [open])

  useEffect(() => {
    if (file === null || typeof globalThis.URL?.createObjectURL !== 'function') {
      setPreviewUrl(null)
      return
    }
    const objectUrl = globalThis.URL.createObjectURL(file)
    setPreviewUrl(objectUrl)
    return () => {
      globalThis.URL.revokeObjectURL(objectUrl)
    }
  }, [file])

  const reset = (): void => {
    setOpen(false)
    setFile(null)
    setDescription('')
    setDecorative(false)
    setError(null)
    if (fileRef.current !== null) fileRef.current.value = ''
    focusEditorInput()
  }

  const submit = (): void => {
    const selectedFile = file
    if (selectedFile === null) {
      setError(labels.imageInvalid)
      fileRef.current?.focus()
      return
    }
    const trimmedDescription = description.trim()
    if (!decorative && trimmedDescription.length === 0) {
      setError(labels.imageDescriptionRequired)
      return
    }
    const accessibility: AuthoredImageAccessibility = decorative
      ? { kind: 'decorative' }
      : { kind: 'described', text: trimmedDescription }
    const action = async (): Promise<void> => {
      const bytes = new Uint8Array(await selectedFile.arrayBuffer())
      if (replace) {
        if (controller.replaceImage === undefined || imageNodeId === undefined) {
          throw new Error('FLOW_IMAGE_REPLACEMENT_UNAVAILABLE')
        }
        await controller.replaceImage(imageNodeId, bytes, accessibility, 'ui')
        return
      }
      if (controller.insertImage === undefined || placement === undefined) {
        throw new Error('FLOW_IMAGE_INSERTION_UNAVAILABLE')
      }
      await controller.insertImage(bytes, placement, accessibility, 'ui')
    }
    setOpen(false)
    setFile(null)
    setError(null)
    void action().finally(focusEditorInput)
  }

  const title = replace ? labels.replaceImage : labels.insertImage
  return (
    <>
      <button
        type="button"
        data-action={replace ? 'editor-image-replace' : 'editor-insert-image'}
        disabled={disabled}
        onMouseDown={(event) => event.preventDefault()}
        onClick={() => setOpen(true)}
      >
        {title}
      </button>
      {open ? (
        <dialog
          open
          role="dialog"
          aria-modal="true"
          aria-labelledby="flowpdf-image-dialog-heading"
          aria-describedby="flowpdf-image-dialog-description"
          className="editor-dialog editor-image-dialog"
          data-image-dialog=""
          onKeyDown={(event) => {
            if (event.key !== 'Escape') return
            event.preventDefault()
            reset()
          }}
        >
          <div role="group" aria-label={title}>
            <h2 id="flowpdf-image-dialog-heading">{title}</h2>
            <p id="flowpdf-image-dialog-description">{labels.imageDialogDescription}</p>
            <label className="editor-dialog-field">
              <span>{labels.imageFile}</span>
              <input
                ref={fileRef}
                data-control="image-file"
                type="file"
                accept="image/png,image/jpeg"
                onChange={(event) => {
                  setFile(event.currentTarget.files?.[0] ?? null)
                  setError(null)
                }}
              />
            </label>
            {previewUrl === null ? null : (
              <img
                className="editor-image-dialog-preview"
                src={previewUrl}
                alt={labels.imagePreview}
                data-image-preview=""
              />
            )}
            <label className="editor-dialog-field editor-dialog-textarea">
              <span>{labels.imageDescription}</span>
              <textarea
                data-control="image-description"
                value={description}
                disabled={decorative}
                onChange={(event) => setDescription(event.currentTarget.value)}
                aria-describedby="flowpdf-image-description-hint"
              />
              <small id="flowpdf-image-description-hint">
                {labels.imageDescriptionHint}
              </small>
            </label>
            <label className="editor-dialog-checkbox">
              <input
                data-control="image-decorative"
                type="checkbox"
                checked={decorative}
                onChange={(event) => setDecorative(event.currentTarget.checked)}
              />
              <span>{labels.imageDecorative}</span>
            </label>
            {error === null ? null : (
              <p role="alert" className="editor-dialog-error" data-image-dialog-error="">
                {error}
              </p>
            )}
            <div className="editor-dialog-actions">
              <button ref={cancelRef} type="button" data-action="editor-image-cancel" onClick={reset}>
                {labels.imageCancel}
              </button>
              <button type="button" data-action="editor-image-confirm" onClick={submit}>
                {labels.imageConfirm}
              </button>
            </div>
          </div>
        </dialog>
      ) : null}
    </>
  )
}

export function ImageAccessibilityDialog({
  block,
  controller,
  locale,
}: {
  readonly block: EditorBlockViewDto
  readonly controller: ImageCommandTarget
  readonly locale: EditorLocale
}) {
  const labels = editorMessages[locale]
  const [open, setOpen] = useState(false)
  const [description, setDescription] = useState('')
  const [decorative, setDecorative] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const cancelRef = useRef<HTMLButtonElement>(null)
  const current = block.image?.accessibility

  const start = (): void => {
    setDescription(current?.kind === 'described' ? current.text : '')
    setDecorative(current?.kind === 'decorative')
    setError(null)
    setOpen(true)
  }
  useEffect(() => {
    if (open) cancelRef.current?.focus()
  }, [open])
  const close = (): void => {
    setOpen(false)
    setError(null)
    focusEditorInput()
  }
  const submit = (): void => {
    const text = description.trim()
    if (!decorative && text.length === 0) {
      setError(labels.imageDescriptionRequired)
      return
    }
    const accessibility: AuthoredImageAccessibility = decorative
      ? { kind: 'decorative' }
      : { kind: 'described', text }
    if (controller.setImageAccessibility === undefined) {
      setError(labels.unavailable)
      return
    }
    setOpen(false)
    void controller.setImageAccessibility(block.nodeId, accessibility, 'ui').finally(focusEditorInput)
  }

  return (
    <>
      <button
        type="button"
        data-action="editor-image-accessibility"
        onMouseDown={(event) => event.preventDefault()}
        onClick={start}
      >
        {labels.imageChangeDescription}
      </button>
      {open ? (
        <dialog
          open
          role="dialog"
          aria-modal="true"
          aria-labelledby="flowpdf-image-accessibility-heading"
          className="editor-dialog editor-image-dialog"
          data-image-accessibility-dialog=""
          onKeyDown={(event) => {
            if (event.key !== 'Escape') return
            event.preventDefault()
            close()
          }}
        >
          <h2 id="flowpdf-image-accessibility-heading">{labels.imageChangeDescription}</h2>
          <label className="editor-dialog-field editor-dialog-textarea">
            <span>{labels.imageDescription}</span>
            <textarea
              data-control="image-description"
              value={description}
              disabled={decorative}
              onChange={(event) => setDescription(event.currentTarget.value)}
            />
          </label>
          <label className="editor-dialog-checkbox">
            <input
              data-control="image-decorative"
              type="checkbox"
              checked={decorative}
              onChange={(event) => setDecorative(event.currentTarget.checked)}
            />
            <span>{labels.imageDecorative}</span>
          </label>
          {error === null ? null : (
            <p role="alert" className="editor-dialog-error" data-image-dialog-error="">
              {error}
            </p>
          )}
          <div className="editor-dialog-actions">
            <button ref={cancelRef} type="button" data-action="editor-image-cancel" onClick={close}>
              {labels.imageCancel}
            </button>
            <button type="button" data-action="editor-image-confirm" onClick={submit}>
              {labels.imageConfirm}
            </button>
          </div>
        </dialog>
      ) : null}
    </>
  )
}

export function ImageActionBar({
  block,
  view,
  controller,
  locale,
  onRequestRemoveImage,
}: ImageActionBarProps) {
  const labels = editorMessages[locale]
  const remove = capabilityFor(view, 'removeImage')
  if (remove.targetNodeId !== block.nodeId || block.image === undefined) return null
  const replace = capabilityFor(view, 'replaceImage')
  const accessibility = capabilityFor(view, 'setImageAccessibility')
  const busy = controller.snapshot().phase === 'pending'
  return (
    <div className="editor-image-actions" data-image-action-bar="" aria-label={labels.imageActions}>
      <ImageInsertDialog
        controller={controller}
        locale={locale}
        imageNodeId={block.nodeId}
        disabled={busy || !replace.enabled}
      />
      <ImageAccessibilityDialog block={block} controller={controller} locale={locale} />
      <button
        type="button"
        className="editor-destructive-action"
        data-action="editor-image-remove"
        disabled={busy || !remove.enabled || remove.confirmation == null}
        onMouseDown={(event) => event.preventDefault()}
        onClick={() => {
          if (remove.confirmation !== null && remove.confirmation !== undefined) {
            onRequestRemoveImage(block.nodeId, remove.confirmation)
          }
        }}
      >
        {labels.imageRemove}
      </button>
      {accessibility.enabled ? null : (
        <small className="editor-control-reason">{labels.imageRequired}</small>
      )}
    </div>
  )
}

export function imageAccessibilityLabel(
  accessibility: ImageAccessibilityDto,
): string {
  if (accessibility.kind === 'described') return accessibility.text
  if (accessibility.kind === 'decorative') return ''
  return ''
}

export function imageSelection(nodeId: string) {
  const position = { nodeId, utf16Offset: 0, affinity: 'forward' as const }
  return { anchor: position, focus: position }
}

export function imageSource(
  controller: ImageCommandTarget,
  contentHash: string,
): string | null {
  return controller.imageSource?.(contentHash) ?? null
}

export type { AuthoredImageAccessibility }

function capabilityFor(view: EditorViewDto, name: string) {
  return (
    view.capabilities.find((candidate) => candidate.name === name) ?? {
      name,
      enabled: false,
      reasonKey: 'unavailable',
    }
  )
}

function focusEditorInput(): void {
  document.querySelector<HTMLTextAreaElement>('[data-editor-input-host]')?.focus()
}
