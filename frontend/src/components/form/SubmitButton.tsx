// Copyright © 2026 Jalapeno Labs

// UI
import { Button, Tooltip } from '@heroui/react'

type Props = {
  // Button label.
  label: string
  // Whether the form is valid and may be submitted.
  isValid: boolean
  // Why the form cannot be submitted yet; shown in a tooltip while invalid.
  invalidReason: string
  // Whether a submit is currently in flight.
  isSubmitting?: boolean
  // Layout classes for the wrapper (for example the top gap, `mt-6`).
  className?: string
  // Stretch to the container width (auth screens); false for a compact,
  // right-aligned action (settings). Defaults to true.
  fullWidth?: boolean
}

// Full-width primary submit button for a `<form>`.
//
// It disables itself until the form is valid, so the form never asks you to submit
// something incomplete. Because a disabled button does not fire hover events, it is
// wrapped in a `<div>` so a Tooltip can still explain why it is disabled; the
// tooltip is suppressed once the form is valid.
export function SubmitButton(props: Props) {
  const { className = '', fullWidth = true } = props
  const widthClass = fullWidth ? 'w-full' : ''

  return <Tooltip content={props.invalidReason} isDisabled={props.isValid} placement='top'>
    <div className={`${widthClass} ${className}`.trim()}>
      <Button
        type='submit'
        className={widthClass}
        color='primary'
        isLoading={props.isSubmitting}
        isDisabled={!props.isValid || props.isSubmitting}
      >
        <span>{ props.label }</span>
      </Button>
    </div>
  </Tooltip>
}
