// Copyright © 2026 Jalapeno Labs

// User interface
import { Input, TextField } from '@heroui/react'

// Misc
import { useInitialFocus } from '../hooks/useInitialFocus'

// Authenticator apps show six digits.
const CODE_LENGTH = 6

type Props = {
  id: string
  label: string
  value: string
  onChange: (value: string) => void
  // Called with the full code as its last digit arrives, so it can submit without a click.
  onComplete?: (value: string) => void
  isInvalid?: boolean
  isDisabled?: boolean
  // Focused once shown, as the one field of the step it is on.
  shouldFocus?: boolean
}

// The six-digit code from an authenticator app, in one plain field, centered.
//
// One plain input rather than a box per digit: password managers fill a code into the field the
// page marks `one-time-code`, and Bitwarden's inline suggestion never reaches the hidden input
// a per-digit widget types into (input-otp issue #152, which HeroUI's InputOTP is built on), so
// its code was offered and then dropped. Anything that is not a digit is dropped as it arrives,
// so a code pasted or filled as "123 456" reads as its six digits. There is no `maxLength`: the
// browser would cut "123 456" to "123 45" before it could be read.
export function AuthenticatorCodeInput(props: Props) {
  const inputRef = useInitialFocus(Boolean(props.shouldFocus))

  return <TextField
    id={props.id}
    aria-label={props.label}
    className='mx-auto w-56'
    autoComplete='one-time-code'
    inputMode='numeric'
    isInvalid={props.isInvalid}
    isDisabled={props.isDisabled}
    value={props.value}
    onChange={(typed) => {
      const code = typed
        .replace(/\D/g, '')
        .slice(0, CODE_LENGTH)
      props.onChange(code)
      if (code.length === CODE_LENGTH && props.value.length < CODE_LENGTH) {
        props.onComplete?.(code)
      }
    }}
  >
    <Input
      ref={inputRef}
      // The letter spacing also follows the last digit; the indent puts as much before the first,
      // so the code sits in the middle of the field.
      className='text-center font-mono text-2xl tracking-[0.4em] indent-[0.4em]'
    />
  </TextField>
}
