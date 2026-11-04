// Copyright © 2026 Jalapeno Labs

// Core
import { beforeEach, describe, expect, it } from 'vitest'

// Misc
import { readsAsSubmit } from './submitOnEnter'

// A key press as the browser delivers it, on a target of the test's choosing.
function press(target: Element, init: KeyboardEventInit = {}) {
  const event = new KeyboardEvent('keydown', { key: 'Enter', bubbles: true, cancelable: true, ...init })
  Object.defineProperty(event, 'target', { value: target })
  return event
}

function element<Tag extends keyof HTMLElementTagNameMap>(tag: Tag, attributes: Record<string, string> = {}) {
  const created = document.createElement(tag)
  for (const [ name, value ] of Object.entries(attributes)) {
    created.setAttribute(name, value)
  }
  document.body.append(created)
  return created
}

describe('readsAsSubmit', () => {
  beforeEach(() => {
    document.body.innerHTML = ''
  })

  it('submits on Enter in a text input, a switch, or a date segment', () => {
    expect(readsAsSubmit(press(element('input')))).toBe(true)
    expect(readsAsSubmit(press(element('input', { type: 'checkbox', role: 'switch' })))).toBe(true)
    expect(readsAsSubmit(press(element('div', { role: 'spinbutton' })))).toBe(true)
  })

  it('adds a line on Enter in a text area, and submits on Ctrl or Cmd with Enter there', () => {
    const textArea = element('textarea')
    expect(readsAsSubmit(press(textArea))).toBe(false)
    expect(readsAsSubmit(press(textArea, { ctrlKey: true }))).toBe(true)
    expect(readsAsSubmit(press(textArea, { metaKey: true }))).toBe(true)
  })

  it('leaves Enter to a button or link, which presses itself', () => {
    expect(readsAsSubmit(press(element('button')))).toBe(false)
    expect(readsAsSubmit(press(element('a', { href: '/somewhere' })))).toBe(false)
    expect(readsAsSubmit(press(element('div', { role: 'button' })))).toBe(false)
  })

  it('leaves Enter to a list box, menu, calendar, or open combobox, which picks what is highlighted', () => {
    const listBox = element('div', { role: 'listbox' })
    const option = document.createElement('div')
    listBox.append(option)
    expect(readsAsSubmit(press(option))).toBe(false)
    expect(readsAsSubmit(press(element('div', { role: 'menu' })))).toBe(false)
    expect(readsAsSubmit(press(element('div', { role: 'grid' })))).toBe(false)
    expect(readsAsSubmit(press(element('input', { 'role': 'combobox', 'aria-expanded': 'true' })))).toBe(false)
    expect(readsAsSubmit(press(element('input', { 'role': 'combobox', 'aria-expanded': 'false' })))).toBe(true)
  })

  it('leaves alone a key a field already used, a held key, a modifier other than Ctrl or Cmd, and composing', () => {
    const input = element('input')
    const used = press(input)
    used.preventDefault()
    expect(readsAsSubmit(used)).toBe(false)
    expect(readsAsSubmit(press(input, { repeat: true }))).toBe(false)
    expect(readsAsSubmit(press(input, { shiftKey: true }))).toBe(false)
    expect(readsAsSubmit(press(input, { altKey: true }))).toBe(false)
    expect(readsAsSubmit(press(input, { isComposing: true }))).toBe(false)
  })

  it('submits only on Enter', () => {
    expect(readsAsSubmit(press(element('input'), { key: 'a' }))).toBe(false)
    expect(readsAsSubmit(press(element('input'), { key: 'Escape' }))).toBe(false)
  })
})
