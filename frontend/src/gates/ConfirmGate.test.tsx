// Copyright © 2026 Jalapeno Labs

import type { ConfirmOptions } from './ConfirmGate'

// Core
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { act, fireEvent, render, screen } from '@testing-library/react'

// User interface
import { ConfirmGate } from './ConfirmGate'

// Misc
import { useConfirm } from '../hooks/useConfirm'

type HarnessProps = {
  options: ConfirmOptions
}

// A page with one button that asks the question.
function Harness(props: HarnessProps) {
  const confirm = useConfirm()
  return <button type='button' onClick={() => confirm(props.options)}>Ask</button>
}

function ask(options: Partial<ConfirmOptions> = {}) {
  const onConfirm = vi.fn()
  const onCancel = vi.fn()
  render(<ConfirmGate>
    <Harness options={{
      title: 'Delete Main key?',
      message: 'Main key and its token are deleted.',
      confirmText: 'Delete',
      onConfirm,
      onCancel,
      ...options,
    }} />
  </ConfirmGate>)
  fireEvent.click(screen.getByRole('button', { name: 'Ask' }))
  return { onConfirm, onCancel }
}

// Lets the countdown's timers run, one second at a time, as the browser would.
function waitSeconds(seconds: number) {
  for (let second = 0; second < seconds; second++) {
    act(() => {
      vi.advanceTimersByTime(1000)
    })
  }
}

function pressEnter(element: Element) {
  fireEvent.keyDown(element, { key: 'Enter' })
  fireEvent.keyUp(element, { key: 'Enter' })
}

describe('ConfirmGate', () => {
  beforeEach(() => {
    vi.useFakeTimers()
  })

  afterEach(() => {
    vi.useRealTimers()
  })

  describe('an irreversible action', () => {
    it('names the item in the body and says in bold that it cannot be undone', () => {
      ask({ isIrreversible: true })

      const dialog = screen.getByRole('alertdialog')
      expect(dialog.textContent).toContain('Main key and its token are deleted.')
      expect(screen.getByText('This can\'t be undone.').tagName).toBe('STRONG')
    })

    it('holds Delete for five seconds, counting down in its label', () => {
      const { onConfirm } = ask({ isIrreversible: true })

      const held = screen.getByRole('button', { name: 'Delete (5)' })
      expect(held.hasAttribute('disabled')).toBe(true)
      fireEvent.click(held)

      waitSeconds(1)
      expect(screen.getByRole('button', { name: 'Delete (4)' })).toBeTruthy()

      waitSeconds(4)
      const ready = screen.getByRole('button', { name: 'Delete' })
      expect(ready.hasAttribute('disabled')).toBe(false)
      expect(onConfirm).not.toHaveBeenCalled()

      fireEvent.click(ready)
      expect(onConfirm).toHaveBeenCalledTimes(1)
    })

    it('starts the count over each time it is asked', () => {
      ask({ isIrreversible: true })
      waitSeconds(5)
      fireEvent.click(screen.getByRole('button', { name: 'Cancel' }))

      fireEvent.click(screen.getByRole('button', { name: 'Ask' }))
      expect(screen.getByRole('button', { name: 'Delete (5)' })).toBeTruthy()
    })

    it('is never confirmed by Enter, even with Delete focused', () => {
      const { onConfirm } = ask({ isIrreversible: true })
      waitSeconds(5)

      const button = screen.getByRole('button', { name: 'Delete' })
      act(() => button.focus())
      pressEnter(button)

      expect(onConfirm).not.toHaveBeenCalled()
      expect(screen.getByRole('alertdialog')).toBeTruthy()
    })
  })

  it('closes on Esc, as a cancel', () => {
    const { onConfirm, onCancel } = ask({ isIrreversible: true })

    fireEvent.keyDown(screen.getByRole('alertdialog'), { key: 'Escape' })

    expect(onCancel).toHaveBeenCalledTimes(1)
    expect(onConfirm).not.toHaveBeenCalled()
  })

  it('does not count down or claim permanence for an action that can be undone', () => {
    ask({ tone: 'danger' })

    expect(screen.getByRole('button', { name: 'Delete' }).hasAttribute('disabled')).toBe(false)
    expect(screen.queryByText('This can\'t be undone.')).toBeNull()
  })

  it('never confirms a danger action on Enter', () => {
    const { onConfirm } = ask({ tone: 'danger' })

    const button = screen.getByRole('button', { name: 'Delete' })
    act(() => button.focus())
    pressEnter(button)

    expect(onConfirm).not.toHaveBeenCalled()
  })

  it('opens a safe action with Confirm focused, so Enter confirms it', () => {
    const { onConfirm } = ask({ confirmText: 'Sign out' })

    // React Aria moves focus into a dialog once it has rendered.
    act(() => {
      vi.advanceTimersByTime(100)
    })
    const button = screen.getByRole('button', { name: 'Sign out' })
    expect(document.activeElement).toBe(button)
    pressEnter(button)

    expect(onConfirm).toHaveBeenCalledTimes(1)
  })

  it('only explains a blocked action, with Got it rather than Close', () => {
    ask({ isIrreversible: true, canConfirm: false })

    expect(screen.queryByRole('button', { name: /Delete/ })).toBeNull()
    expect(screen.queryByText('This can\'t be undone.')).toBeNull()
    expect(screen.getByRole('button', { name: 'Got it' })).toBeTruthy()
    expect(document.querySelector('[data-slot="alert-dialog-icon"]')?.className).toMatch(/danger/)
  })

  describe('a switch that makes the action irreversible', () => {
    const switches = [{ id: 'permanently', label: 'Also delete its files', makesIrreversible: true }]

    it('starts off, so the action is the one that can be undone', () => {
      const { onConfirm } = ask({ tone: 'danger', switches })

      const toggle = screen.getByRole('switch', { name: 'Also delete its files' }) as HTMLInputElement
      expect(toggle.checked).toBe(false)
      expect(screen.queryByText('This can\'t be undone.')).toBeNull()

      fireEvent.click(screen.getByRole('button', { name: 'Delete' }))
      expect(onConfirm).toHaveBeenCalledWith(new Set())
    })

    it('turned on, says so, counts down, and confirms with it on', () => {
      const { onConfirm } = ask({ tone: 'danger', switches })

      fireEvent.click(screen.getByRole('switch', { name: 'Also delete its files' }))
      expect(screen.getByText('This can\'t be undone.')).toBeTruthy()
      expect(screen.getByRole('button', { name: 'Delete (5)' }).hasAttribute('disabled')).toBe(true)

      waitSeconds(5)
      fireEvent.click(screen.getByRole('button', { name: 'Delete' }))
      expect(onConfirm).toHaveBeenCalledWith(new Set([ 'permanently' ]))
    })

    it('counts down again when turned off and on', () => {
      ask({ tone: 'danger', switches })
      const toggle = screen.getByRole('switch', { name: 'Also delete its files' })

      fireEvent.click(toggle)
      waitSeconds(5)
      fireEvent.click(toggle)
      fireEvent.click(toggle)

      expect(screen.getByRole('button', { name: 'Delete (5)' })).toBeTruthy()
    })
  })
})
