// Copyright © 2026 Jalapeno Labs

// Core
import { describe, expect, it, vi } from 'vitest'
import { fireEvent, render, screen } from '@testing-library/react'

// Misc
import { PasswordInput } from './PasswordInput'
import { SecretInput } from './SecretInput'

// #166: every secret field hides what is typed and has a toggle to show it, and the checklist under
// a new password lists every requirement the form waits on, strength included.

describe('SecretInput', () => {
  it('hides the value until the eye shows it, and hides it again', () => {
    render(<SecretInput id='token' label='API token' value='ghp_secret' onChange={vi.fn()} />)
    const field = screen.getByLabelText(/API token/)
    expect(field.getAttribute('type')).toBe('password')

    fireEvent.click(screen.getByRole('button', { name: 'Show' }))
    expect(field.getAttribute('type')).toBe('text')

    fireEvent.click(screen.getByRole('button', { name: 'Hide' }))
    expect(field.getAttribute('type')).toBe('password')
  })

  it('keeps a pasted secret out of the browser\'s saved passwords unless told otherwise', () => {
    const { rerender } = render(<SecretInput id='token' label='API token' value='' onChange={vi.fn()} />)
    expect(screen.getByLabelText(/API token/).getAttribute('autocomplete')).toBe('off')

    rerender(<SecretInput id='password' label='Password' value='' autoComplete='current-password' onChange={vi.fn()} />)
    expect(screen.getByLabelText(/Password/).getAttribute('autocomplete')).toBe('current-password')
  })
})

describe('PasswordInput', () => {
  const unmet = { length: false, uppercase: false, special: false, strength: false }

  it('lists strength with the other requirements', () => {
    render(<PasswordInput
      id='password'
      label='Password'
      value='weak'
      onChange={vi.fn()}
      assessment={{ requirements: { ...unmet, length: true }, strength: null, isAcceptable: false }}
    />)
    const items = screen.getAllByRole('listitem').map((item) => item.textContent)
    expect(items).toEqual([
      'At least 12 characters',
      'An uppercase letter',
      'A symbol or punctuation mark',
      'Hard to guess',
    ])
  })

  it('marks a met requirement in green with a check, and an unmet one with a red X', () => {
    render(<PasswordInput
      id='password'
      label='Password'
      value='weak'
      onChange={vi.fn()}
      assessment={{ requirements: { ...unmet, length: true }, strength: null, isAcceptable: false }}
    />)
    const [ length, uppercase ] = screen.getAllByRole('listitem')
    expect(length.className).toContain('text-success')
    expect(uppercase.className).not.toContain('text-success')
    expect(uppercase.querySelector('svg')?.getAttribute('class')).toContain('text-danger')
  })
})
