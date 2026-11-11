// Copyright © 2026 Jalapeno Labs

// Core
import { useCallback, useState } from 'react'
import { useTranslation } from 'react-i18next'

// User interface
import { Button, Description, Form, Label, NumberField } from '@heroui/react'
import { LuCheck } from 'react-icons/lu'

// Misc
import { CAPTURE_OFFSET_MAX_MS } from '../../../api/routes/meetingRoutes'

const MILLISECONDS_PER_SECOND = 1000
const OFFSET_MAX_SECONDS = CAPTURE_OFFSET_MAX_MS / MILLISECONDS_PER_SECOND

// Each step of the field's arrows, and what a typed value lands on: a tenth of a second is about
// the least a person hears two recordings drift apart by, so finer is noise and coarser is clumsy.
const STEP_SECONDS = 0.1

type Props = {
  captureId: string
  // Where the capture starts now, in milliseconds into the meeting.
  offsetMs: number
  // Resolves whether the move applied; one refused keeps the field open with what was typed.
  onApply: (offsetMs: number) => Promise<boolean>
  onCancel: () => void
}

// A capture's start on the meeting's clock, opened in place to move it by hand (docs/meetings.md,
// Aligning, Set by hand): seconds into the meeting, below zero for a recording that started
// early, applied with Enter or Move, and Esc or Cancel to leave it where it is. The field holds
// the offset within twelve hours either way, as the API does.
export function CaptureOffsetEditor(props: Props) {
  const { t } = useTranslation([ 'meetings', 'common' ])
  const [ seconds, setSeconds ] = useState(props.offsetMs / MILLISECONDS_PER_SECOND)
  const [ isPending, setIsPending ] = useState(false)
  // Cleared, the field holds no number at all.
  const offsetMs = Math.round(seconds * MILLISECONDS_PER_SECOND)
  const canApply = Number.isFinite(seconds) && offsetMs !== props.offsetMs && !isPending

  // Focused as it opens, with the offset selected, so typing replaces it.
  const focusField = useCallback((field: HTMLInputElement | null) => {
    field?.focus()
    field?.select()
  }, [])

  async function apply() {
    setIsPending(true)
    try {
      await props.onApply(offsetMs)
    }
    finally {
      setIsPending(false)
    }
  }

  // Enter is the browser's own submit from the field, which comes after the field has taken the
  // number typed: `submitOnEnter` would submit on the way down, before it has.
  return <Form
    className='rounded-lg border border-border bg-surface p-3 shadow-sm'
    validationBehavior='aria'
    onKeyDownCapture={(event) => {
      // Esc leaves the recording where it is; the page's own Esc must not go up a level.
      if (event.key === 'Escape' && !isPending) {
        event.preventDefault()
        event.stopPropagation()
        props.onCancel()
      }
    }}
    onSubmit={(event) => {
      event.preventDefault()
      if (canApply) {
        void apply()
      }
    }}
  >
    <NumberField
      id={`capture-offset-${props.captureId}`}
      className='w-full'
      value={seconds}
      onChange={setSeconds}
      minValue={-OFFSET_MAX_SECONDS}
      maxValue={OFFSET_MAX_SECONDS}
      step={STEP_SECONDS}
      formatOptions={{ maximumFractionDigits: 1 }}
      isDisabled={isPending}
    >
      <Label className='text-xs'>{t('review.captures.moveLabel')}</Label>
      <NumberField.Group>
        <NumberField.DecrementButton />
        <NumberField.Input ref={focusField} />
        <NumberField.IncrementButton />
      </NumberField.Group>
      <Description className='text-xs'>{t('review.captures.moveHint')}</Description>
    </NumberField>
    <div className='mt-2 flex justify-end gap-2'>
      <Button size='sm' variant='ghost' isDisabled={isPending} onPress={props.onCancel}>
        <span>{t('common:actions.cancel')}</span>
      </Button>
      <Button type='submit' size='sm' isDisabled={!canApply && !isPending} isPending={isPending}>
        <LuCheck className='size-4' aria-hidden />
        <span>{t('review.captures.apply')}</span>
      </Button>
    </div>
  </Form>
}
