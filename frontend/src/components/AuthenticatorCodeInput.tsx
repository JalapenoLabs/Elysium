// Copyright © 2026 Jalapeno Labs

// User interface
import { InputOTP, REGEXP_ONLY_DIGITS } from '@heroui/react'

// Authenticator apps show six digits.
const CODE_LENGTH = 6

type Props = {
  label: string
  value: string
  onChange: (value: string) => void
  // Called with the full code as its last digit is typed, so it can submit without a click.
  onComplete?: (value: string) => void
  isInvalid?: boolean
  isDisabled?: boolean
  autoFocus?: boolean
}

// The six-digit code from an authenticator app, one box per digit. Pasting a whole code
// fills every box.
export function AuthenticatorCodeInput(props: Props) {
  return <InputOTP
    aria-label={props.label}
    maxLength={CODE_LENGTH}
    pattern={REGEXP_ONLY_DIGITS}
    inputMode='numeric'
    autoComplete='one-time-code'
    autoFocus={props.autoFocus}
    isInvalid={props.isInvalid}
    isDisabled={props.isDisabled}
    value={props.value}
    onChange={props.onChange}
    onComplete={props.onComplete}
  >
    <InputOTP.Group>
      <InputOTP.Slot index={0} />
      <InputOTP.Slot index={1} />
      <InputOTP.Slot index={2} />
    </InputOTP.Group>
    <InputOTP.Separator />
    <InputOTP.Group>
      <InputOTP.Slot index={3} />
      <InputOTP.Slot index={4} />
      <InputOTP.Slot index={5} />
    </InputOTP.Group>
  </InputOTP>
}
