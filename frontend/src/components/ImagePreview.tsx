// Copyright © 2026 Jalapeno Labs

import type { KeyboardEvent, ReactNode } from 'react'

// Core
import { useTranslation } from 'react-i18next'

// User interface
import { Modal, Tooltip, useOverlayState } from '@heroui/react'

// Misc
import { IMAGE_PREVIEW_HOVER_DELAY_MS } from '../constants'

type Props = {
  src: string
  // Describes the image, for the enlarged views.
  alt: string
  // What shows in place: usually a small rendering of the same image.
  children: ReactNode
  className?: string
}

// Wraps a small image so it can be seen larger. Resting the pointer on it for two seconds
// shows a larger copy beside it; clicking it, or pressing Enter or Space while it has
// focus, opens it fullscreen.
export function ImagePreview(props: Props) {
  const { t } = useTranslation('common')
  const fullscreenState = useOverlayState()

  function onKeyDown(event: KeyboardEvent) {
    if (event.key !== 'Enter' && event.key !== ' ') {
      return
    }
    event.preventDefault()
    fullscreenState.open()
  }

  return <>
    <Tooltip delay={IMAGE_PREVIEW_HOVER_DELAY_MS} closeDelay={0}>
      {/* The trigger is focusable and announced as a button, and opens the fullscreen view. */}
      <Tooltip.Trigger
        aria-label={t('imagePreview.open', { name: props.alt })}
        className={`cursor-zoom-in ${props.className ?? ''}`}
        onClick={fullscreenState.open}
        onKeyDown={onKeyDown}
      >
        {props.children}
      </Tooltip.Trigger>
      <Tooltip.Content placement='right' className='max-w-none p-1'>
        <img
          src={props.src}
          alt={props.alt}
          className='max-h-80 max-w-80 rounded-md object-contain'
        />
      </Tooltip.Content>
    </Tooltip>

    <Modal.Backdrop
      variant='blur'
      isOpen={fullscreenState.isOpen}
      onOpenChange={fullscreenState.setOpen}
    >
      <Modal.Container size='full'>
        <Modal.Dialog className='bg-transparent'>
          <Modal.CloseTrigger />
          <Modal.Header className='sr-only'>
            <Modal.Heading>{props.alt}</Modal.Heading>
          </Modal.Header>
          {/* Clicking around the image closes the view; the image itself does not. */}
          <Modal.Body
            className='grid size-full cursor-zoom-out place-items-center p-6'
            onClick={(event) => {
              if (event.target === event.currentTarget) {
                fullscreenState.close()
              }
            }}
          >
            <img
              src={props.src}
              alt={props.alt}
              className='max-h-full max-w-full cursor-default rounded-xl object-contain shadow-2xl'
            />
          </Modal.Body>
        </Modal.Dialog>
      </Modal.Container>
    </Modal.Backdrop>
  </>
}
