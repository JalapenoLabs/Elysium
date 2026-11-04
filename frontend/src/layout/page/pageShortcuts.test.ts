// Copyright © 2026 Jalapeno Labs

// Core
import { afterEach, describe, expect, it } from 'vitest'

// Misc
import { readPageShortcut } from './pageShortcuts'

function press(init: KeyboardEventInit, target: Element = document.body) {
  const event = new KeyboardEvent('keydown', { bubbles: true, cancelable: true, ...init })
  Object.defineProperty(event, 'target', { value: target })
  return event
}

describe('readPageShortcut', () => {
  afterEach(() => {
    document.body.innerHTML = ''
  })

  it('reads Ctrl+S and Cmd+S as save', () => {
    expect(readPageShortcut(press({ key: 's', ctrlKey: true }))).toBe('save')
    expect(readPageShortcut(press({ key: 'S', metaKey: true }))).toBe('save')
    expect(readPageShortcut(press({ key: 's', ctrlKey: true, shiftKey: true }))).toBeNull()
    expect(readPageShortcut(press({ key: 's' }))).toBeNull()
  })

  it('reads a plain Esc as leave, even while typing', () => {
    const input = document.createElement('input')
    document.body.append(input)
    expect(readPageShortcut(press({ key: 'Escape' }, input))).toBe('leave')
    expect(readPageShortcut(press({ key: 'Escape', shiftKey: true }))).toBeNull()
  })

  it('leaves Esc to a component that already used it', () => {
    const event = press({ key: 'Escape' })
    event.preventDefault()
    expect(readPageShortcut(event)).toBeNull()
  })

  it('leaves Esc to an open menu, list box, or dialog', () => {
    const menu = document.createElement('div')
    menu.setAttribute('role', 'menu')
    const item = document.createElement('div')
    menu.append(item)
    document.body.append(menu)
    expect(readPageShortcut(press({ key: 'Escape' }, item))).toBeNull()

    menu.remove()
    const dialog = document.createElement('div')
    dialog.setAttribute('role', 'dialog')
    document.body.append(dialog)
    // Focus can sit outside a modal, as it does the moment one opens.
    expect(readPageShortcut(press({ key: 'Escape' }))).toBeNull()
  })
})
