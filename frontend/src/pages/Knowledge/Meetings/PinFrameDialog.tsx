// Copyright © 2026 Jalapeno Labs

import type { Still } from './useMeetingPlayer'

// Core
import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'

// User interface
import { Button, Form, Label, Modal, TextArea, TextField } from '@heroui/react'

// Misc
import { formatMediaPosition } from '../../../format/durations'
import { useInitialFocus } from '../../../hooks/useInitialFocus'

type Props = {
  still: Still
  // What it is pinned over, read to the person: the words selected, or the word spoken then.
  pinnedOver: string | null
  captionMaxCharacters: number
  // Pins it with its caption; resolves whether it did, the dialog staying open with what was typed
  // when it did not.
  onPin: (caption: string) => Promise<boolean>
  onClose: () => void
}

// Pin a frame (docs/meeting-review.md, Attachments and frames): the still the player showed as F
// was pressed, and what it shows, in the person's words. Ctrl+Enter pins, as a comment posts.
export function PinFrameDialog(props: Props) {
  const { t } = useTranslation([ 'meetings', 'common' ])
  const [ caption, setCaption ] = useState('')
  const [ isPinning, setIsPinning ] = useState(false)
  const [ preview, setPreview ] = useState<string | null>(null)
  const fieldRef = useInitialFocus()
  const trimmed = caption.trim()
  const canPin = Boolean(trimmed) && trimmed.length <= props.captionMaxCharacters && !isPinning

  // The still shows from a blob URL, which the page's policy allows for an image, released once the
  // dialog is done with it.
  useEffect(() => {
    const address = URL.createObjectURL(props.still.picture)
    setPreview(address)
    return () => URL.revokeObjectURL(address)
  }, [ props.still.picture ])

  async function pin() {
    if (!canPin) {
      return
    }
    setIsPinning(true)
    const pinned = await props.onPin(trimmed)
    setIsPinning(false)
    if (pinned) {
      props.onClose()
    }
  }

  return <Modal.Backdrop
    isOpen
    onOpenChange={(isOpen) => {
      if (!isOpen) {
        props.onClose()
      }
    }}
  >
    <Modal.Container>
      <Modal.Dialog className='sm:max-w-xl'>
        <Modal.CloseTrigger />
        <Modal.Header>
          <Modal.Heading>{
            t('review.files.pinTitle', { moment: formatMediaPosition(props.still.meetingMs) })
          }</Modal.Heading>
        </Modal.Header>
        <Form
          validationBehavior='aria'
          onSubmit={(event) => {
            event.preventDefault()
            void pin()
          }}
        >
          <Modal.Body className='mt-2 flex flex-col gap-3'>
            {preview && <img
              src={preview}
              alt={t('review.files.stillAlt')}
              className='max-h-72 w-full rounded-lg border border-border bg-black object-contain'
            />}
            {props.pinnedOver && <p className='line-clamp-3 text-xs opacity-70'>{
              t('review.files.pinnedOverWords', { words: props.pinnedOver })
            }</p>}
            <TextField id='meeting-frame-caption' value={caption} onChange={setCaption}>
              <Label>{t('review.files.caption')}</Label>
              <TextArea
                ref={fieldRef}
                rows={3}
                maxLength={props.captionMaxCharacters}
                onKeyDown={(event) => {
                  if (event.key === 'Enter' && (event.ctrlKey || event.metaKey)) {
                    event.preventDefault()
                    void pin()
                  }
                }}
              />
            </TextField>
          </Modal.Body>
          <Modal.Footer>
            <Button slot='close' variant='tertiary'>
              <span>{t('common:actions.cancel')}</span>
            </Button>
            <Button type='submit' isDisabled={!canPin} isPending={isPinning}>
              <span>{t('review.files.pin')}</span>
            </Button>
          </Modal.Footer>
        </Form>
      </Modal.Dialog>
    </Modal.Container>
  </Modal.Backdrop>
}
