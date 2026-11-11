// Copyright © 2026 Jalapeno Labs

// Core
import { describe, expect, it, vi } from 'vitest'
import { render, screen } from '@testing-library/react'
import { useState } from 'react'

// User interface
import { AuthenticatorCodeInput } from './AuthenticatorCodeInput'

type HarnessProps = {
  onComplete: (value: string) => void
  initialValue?: string
}

// The field as its pages hold it: its value in state, given back on every change.
function Harness(props: HarnessProps) {
  const [ code, setCode ] = useState(props.initialValue ?? '')

  return <AuthenticatorCodeInput
    shouldFocus
    id='authenticator-code'
    label='Authenticator code'
    value={code}
    onChange={setCode}
    onComplete={props.onComplete}
  />
}

// Fills the field the way Bitwarden does (`insert-autofill-content.service.ts`): a click and
// focus, keydown and keyup, the value assigned through the element's own setter, then keydown,
// keyup, and bubbling `input` and `change` events. No keystroke carries a digit.
function fillLikeBitwarden(field: HTMLInputElement, value: string) {
  field.click()
  field.focus()
  field.dispatchEvent(new KeyboardEvent('keydown', { bubbles: true }))
  field.dispatchEvent(new KeyboardEvent('keyup', { bubbles: true }))
  const setValue = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value')?.set
  setValue?.call(field, value)
  field.dispatchEvent(new KeyboardEvent('keydown', { bubbles: true }))
  field.dispatchEvent(new KeyboardEvent('keyup', { bubbles: true }))
  field.dispatchEvent(new Event('input', { bubbles: true }))
  field.dispatchEvent(new Event('change', { bubbles: true }))
}

describe('AuthenticatorCodeInput', () => {
  // Regression: Bitwarden's inline suggestion offered the code and then dropped it, because the
  // field was input-otp's hidden single input (input-otp issue #152). It must stay one plain
  // input, the field itself, with no per-digit widget around it.
  it('takes focus when asked, as a plain field password managers know holds a one-time code', () => {
    render(<Harness onComplete={() => {}} />)

    const field = screen.getByRole('textbox', { name: 'Authenticator code' })
    expect(document.activeElement).toBe(field)
    expect(screen.getAllByRole('textbox')).toHaveLength(1)
    expect(document.querySelector('[data-input-otp]')).toBeNull()
    expect(field.getAttribute('autocomplete')).toBe('one-time-code')
    expect(field.getAttribute('inputmode')).toBe('numeric')
  })

  it('keeps a code a password manager fills, and completes it', () => {
    const onComplete = vi.fn()
    render(<Harness onComplete={onComplete} />)
    const field = screen.getByRole<HTMLInputElement>('textbox', { name: 'Authenticator code' })

    fillLikeBitwarden(field, '482913')

    expect(field.value).toBe('482913')
    expect(onComplete).toHaveBeenCalledExactlyOnceWith('482913')
  })

  it('reads a code filled or pasted with a space as its six digits', () => {
    const onComplete = vi.fn()
    render(<Harness onComplete={onComplete} />)
    const field = screen.getByRole<HTMLInputElement>('textbox', { name: 'Authenticator code' })

    fillLikeBitwarden(field, '482 913')

    expect(field.value).toBe('482913')
    expect(onComplete).toHaveBeenCalledExactlyOnceWith('482913')
  })

  it('drops what is not a digit and stops at six', () => {
    const onComplete = vi.fn()
    render(<Harness onComplete={onComplete} />)
    const field = screen.getByRole<HTMLInputElement>('textbox', { name: 'Authenticator code' })

    fillLikeBitwarden(field, 'a1b2c3')
    expect(field.value).toBe('123')
    expect(onComplete).not.toHaveBeenCalled()

    fillLikeBitwarden(field, '12345678')
    expect(field.value).toBe('123456')
    expect(onComplete).toHaveBeenCalledExactlyOnceWith('123456')
  })

  it('completes only as the last digit arrives, not on a change to a full code', () => {
    const onComplete = vi.fn()
    render(<Harness initialValue='123456' onComplete={onComplete} />)
    const field = screen.getByRole<HTMLInputElement>('textbox', { name: 'Authenticator code' })

    fillLikeBitwarden(field, '654321')

    expect(field.value).toBe('654321')
    expect(onComplete).not.toHaveBeenCalled()
  })
})
