// Copyright © 2026 Jalapeno Labs

import type { PointerEvent } from 'react'
import type { AnnotationSource, DrawingStroke, DrawingTool, PenColor } from './annotation'

// Core
import { useEffect, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'

// Redux
import { codingSessionUpserted } from '../../store/codingSessionsSlice'
import { useAppDispatch } from '../../store/hooks'
import { studioFeedbackCreated } from '../../store/studioFeedbackSlice'

// User interface
import {
  Button,
  Description,
  Label,
  Modal,
  Spinner,
  TextArea,
  TextField,
  toast,
  ToggleButton,
  ToggleButtonGroup,
  Tooltip,
} from '@heroui/react'
import { LuEraser, LuMoveUpRight, LuPencil, LuSendHorizontal, LuUndo2 } from 'react-icons/lu'

// Misc
import { getApiErrorMessage } from '../../api/errors'
import { sendStudioTurn } from '../../api/routes/studioRoutes'
import { DRAWING_TOOLS, getPenSize, PEN_COLORS, toImagePoint } from './annotation'
import { drawAnnotation } from './drawAnnotation'

const toolIcons = {
  pen: LuPencil,
  arrow: LuMoveUpRight,
} as const

const toolLabelKeys = {
  pen: 'annotate.pen',
  arrow: 'annotate.arrow',
} as const satisfies Record<DrawingTool, string>

const colorLabelKeys = {
  red: 'annotate.colorNames.red',
  yellow: 'annotate.colorNames.yellow',
  blue: 'annotate.colorNames.blue',
} as const satisfies Record<PenColor, string>

// The swatches show the same theme tokens the pens draw with.
const colorSwatchClassNames = {
  red: 'bg-danger',
  yellow: 'bg-warning',
  blue: 'bg-accent',
} as const satisfies Record<PenColor, string>

type Props = {
  itemId: string
  source: AnnotationSource
  onClose: () => void
}

// Draws over a view and sends it with a prompt. The canvas holds the view at its full size
// with the strokes painted over it, so what is sent is exactly what was drawn.
export function AnnotateModal(props: Props) {
  const { t } = useTranslation([ 'studio', 'common' ])
  const dispatch = useAppDispatch()
  const canvasRef = useRef<HTMLCanvasElement>(null)
  const [ image, setImage ] = useState<HTMLImageElement | null>(null)
  const [ hasLoadFailed, setHasLoadFailed ] = useState(false)
  const [ strokes, setStrokes ] = useState<DrawingStroke[]>([])
  // The stroke under the pointer, until it lifts.
  const [ draft, setDraft ] = useState<DrawingStroke | null>(null)
  const [ tool, setTool ] = useState<DrawingTool>('pen')
  const [ color, setColor ] = useState<PenColor>('red')
  const [ prompt, setPrompt ] = useState('')
  const [ isSending, setIsSending ] = useState(false)

  useEffect(() => {
    const loading = new Image()
    loading.onload = () => setImage(loading)
    loading.onerror = (event) => {
      console.debug('AnnotateModal could not load the view to draw on', { event, url: props.source.imageUrl })
      setHasLoadFailed(true)
    }
    loading.src = props.source.imageUrl
  }, [ props.source.imageUrl ])

  const penSize = image
    ? getPenSize(image.naturalWidth, image.naturalHeight)
    : 0

  useEffect(() => {
    const context = canvasRef.current?.getContext('2d')
    if (!image || !context) {
      return
    }
    const shownStrokes = draft
      ? [ ...strokes, draft ]
      : strokes
    drawAnnotation(context, image, shownStrokes, penSize)
  }, [ image, strokes, draft, penSize ])

  function pointFrom(event: PointerEvent<HTMLCanvasElement>) {
    // Pressure is only meaningful from a stylus; 0.5 is the Pointer Events default otherwise.
    const pressure = event.pointerType === 'pen'
      ? event.pressure
      : 0.5
    return toImagePoint(
      event.clientX,
      event.clientY,
      pressure,
      event.currentTarget.getBoundingClientRect(),
      event.currentTarget.width,
      event.currentTarget.height,
    )
  }

  function startStroke(event: PointerEvent<HTMLCanvasElement>) {
    event.currentTarget.setPointerCapture(event.pointerId)
    setDraft({
      tool,
      color,
      points: [ pointFrom(event) ],
      hasPressure: event.pointerType === 'pen',
    })
  }

  function continueStroke(event: PointerEvent<HTMLCanvasElement>) {
    if (!draft) {
      return
    }
    const point = pointFrom(event)
    // A pen keeps every point it passes; an arrow keeps its tail and moves its tip.
    const points = draft.tool === 'pen'
      ? [ ...draft.points, point ]
      : [ draft.points[0], point ]
    setDraft({ ...draft, points })
  }

  function endStroke() {
    if (!draft) {
      return
    }
    // An arrow needs a tip apart from its tail; a click without a drag draws nothing.
    if (draft.tool === 'pen' || draft.points.length > 1) {
      setStrokes((previous) => [ ...previous, draft ])
    }
    setDraft(null)
  }

  async function send() {
    const canvas = canvasRef.current
    if (!canvas || !prompt.trim()) {
      console.debug('AnnotateModal was asked to send without a drawing or a prompt')
      return
    }

    setIsSending(true)
    try {
      const annotated = await new Promise<Blob | null>((resolve) => canvas.toBlob(resolve, 'image/png'))
      if (!annotated) {
        throw new Error('The drawing could not be flattened to PNG')
      }
      const response = await sendStudioTurn(props.itemId, {
        prompt: prompt.trim(),
        sourceAssetId: props.source.sourceAssetId,
        cameraOrbit: props.source.cameraOrbit,
        annotated,
        capture: props.source.capture,
      })
      dispatch(codingSessionUpserted(response.session))
      if (response.feedback) {
        dispatch(studioFeedbackCreated(response.feedback))
      }
      toast.success(t('toasts.sent'))
      props.onClose()
    }
    catch (error) {
      const message = getApiErrorMessage(error)
      if (!message) {
        console.debug('AnnotateModal failed to send a drawn prompt', { error, itemId: props.itemId })
      }
      toast.danger(t('toasts.sendFailed'), {
        description: message ?? t('common:errors.unexpected'),
      })
    }
    finally {
      setIsSending(false)
    }
  }

  return <Modal.Backdrop
    isOpen
    onOpenChange={(isOpen) => {
      // Closing mid-send would hide whether the prompt went.
      if (!isOpen && !isSending) {
        props.onClose()
      }
    }}
  >
    <Modal.Container size='cover'>
      <Modal.Dialog className='flex h-full flex-col'>
        <Modal.CloseTrigger />
        <Modal.Header>
          <Modal.Heading>{t('annotate.title')}</Modal.Heading>
        </Modal.Header>

        <Modal.Body className='flex min-h-0 flex-1 flex-col gap-3'>
          {/* Tools */}
          <div className='flex flex-wrap items-center gap-2' role='toolbar' aria-label={t('annotate.tools')}>
            <ToggleButtonGroup
              size='sm'
              aria-label={t('annotate.tools')}
              selectionMode='single'
              disallowEmptySelection
              selectedKeys={[ tool ]}
              onSelectionChange={(keys) => {
                // The group disallows an empty selection, so one tool is always chosen.
                const [ key ] = [ ...keys ]
                const nextTool = DRAWING_TOOLS.find((candidate) => candidate === key)
                if (nextTool) {
                  setTool(nextTool)
                }
              }}
            >{
                DRAWING_TOOLS.map((drawingTool, index) => {
                  const Icon = toolIcons[drawingTool]
                  return <ToggleButton key={drawingTool} id={drawingTool}>
                    {index > 0 && <ToggleButtonGroup.Separator />}
                    <Icon className='size-4' aria-hidden />
                    <span>{t(toolLabelKeys[drawingTool])}</span>
                  </ToggleButton>
                })
              }</ToggleButtonGroup>

            <div className='flex items-center gap-1' role='group' aria-label={t('annotate.colors')}>{
              PEN_COLORS.map((penColor) => <Tooltip key={penColor} delay={300}>
                <Button
                  isIconOnly
                  size='sm'
                  variant={penColor === color
                    ? 'secondary'
                    : 'ghost'}
                  aria-label={t(colorLabelKeys[penColor])}
                  aria-pressed={penColor === color}
                  onPress={() => setColor(penColor)}
                >
                  <span className={`size-4 rounded-full ${colorSwatchClassNames[penColor]}`} aria-hidden />
                </Button>
                <Tooltip.Content>
                  <span>{t(colorLabelKeys[penColor])}</span>
                </Tooltip.Content>
              </Tooltip>)
            }</div>

            <div className='ml-auto flex items-center gap-2'>
              <Button
                size='sm'
                variant='ghost'
                isDisabled={!strokes.length}
                onPress={() => setStrokes((previous) => previous.slice(0, -1))}
              >
                <LuUndo2 className='size-4' aria-hidden />
                <span>{t('annotate.undo')}</span>
              </Button>
              <Button
                size='sm'
                variant='ghost'
                isDisabled={!strokes.length}
                onPress={() => setStrokes([])}
              >
                <LuEraser className='size-4' aria-hidden />
                <span>{t('annotate.clear')}</span>
              </Button>
            </div>
          </div>

          {/* The view, scaled to fit, under the drawing */}
          <div className='grid min-h-0 flex-1 place-items-center overflow-hidden rounded-xl bg-surface-secondary'>
            {!image && !hasLoadFailed && <Spinner />}
            {hasLoadFailed && <p className='text-sm text-danger'>{t('annotate.loadError')}</p>}
            <canvas
              ref={canvasRef}
              aria-label={t('annotate.canvasLabel')}
              width={image?.naturalWidth ?? 0}
              height={image?.naturalHeight ?? 0}
              className={image
                ? 'h-auto max-h-full w-auto max-w-full cursor-crosshair touch-none'
                : 'hidden'}
              onPointerDown={startStroke}
              onPointerMove={continueStroke}
              onPointerUp={endStroke}
              onPointerCancel={endStroke}
            />
          </div>
        </Modal.Body>

        <Modal.Footer className='flex-col items-stretch gap-2 sm:flex-row sm:items-end'>
          <TextField
            autoFocus
            className='flex-1'
            value={prompt}
            onChange={setPrompt}
          >
            <Label>{t('annotate.prompt')}</Label>
            <TextArea rows={2} />
            <Description>{t('annotate.promptHint')}</Description>
          </TextField>
          <Button
            className='sm:mb-6'
            isDisabled={!image || !prompt.trim()}
            isPending={isSending}
            onPress={() => void send()}
          >
            <LuSendHorizontal className='size-4' aria-hidden />
            <span>{t('annotate.send')}</span>
          </Button>
        </Modal.Footer>
      </Modal.Dialog>
    </Modal.Container>
  </Modal.Backdrop>
}
