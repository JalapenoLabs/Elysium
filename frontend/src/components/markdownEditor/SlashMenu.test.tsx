// Copyright © 2026 Jalapeno Labs

import type { RefObject } from 'react'

// Core
import { act, render, screen } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'

// User interface
import { SlashMenu } from './SlashMenu'

type Keys = RefObject<((event: KeyboardEvent) => boolean) | null>

function renderMenu(query: string, keysRef: Keys, onChoose = vi.fn()) {
  render(<SlashMenu
    state={{ query, range: { from: 1, to: 2 }, anchor: null }}
    keysRef={keysRef}
    onChoose={onChoose}
  />)
  return onChoose
}

function press(keysRef: Keys, key: string) {
  let isUsed = false
  act(() => {
    isUsed = keysRef.current?.(new KeyboardEvent('keydown', { key })) ?? false
  })
  return isUsed
}

describe('SlashMenu', () => {
  beforeEach(() => {
    // jsdom lays nothing out, so has nothing to scroll.
    Element.prototype.scrollIntoView = () => undefined
  })

  it('narrows to what was typed, by name or by a search word', () => {
    const keysRef: Keys = { current: null }
    renderMenu('hr', keysRef)

    expect(screen.getAllByRole('option').map((option) => option.textContent)).toEqual([ 'Divider' ])
  })

  it('chooses the highlighted block with Enter, moved by the arrow keys', () => {
    const keysRef: Keys = { current: null }
    const onChoose = renderMenu('heading', keysRef)

    expect(press(keysRef, 'ArrowDown')).toBe(true)
    expect(press(keysRef, 'Enter')).toBe(true)

    expect(onChoose).toHaveBeenCalledWith(expect.objectContaining({ id: 'heading2' }))
  })

  it('wraps around the list', () => {
    const keysRef: Keys = { current: null }
    const onChoose = renderMenu('heading', keysRef)

    press(keysRef, 'ArrowUp')
    press(keysRef, 'Enter')

    expect(onChoose).toHaveBeenCalledWith(expect.objectContaining({ id: 'heading3' }))
  })

  it('says so when nothing matches, and leaves Enter to the editor', () => {
    const keysRef: Keys = { current: null }
    const onChoose = renderMenu('nothing like this', keysRef)

    expect(screen.getByText('No matching block')).toBeTruthy()
    expect(press(keysRef, 'Enter')).toBe(false)
    expect(onChoose).not.toHaveBeenCalled()
  })

  it('leaves every other key to the editor, so typing goes on narrowing it', () => {
    const keysRef: Keys = { current: null }
    renderMenu('', keysRef)

    expect(press(keysRef, 'a')).toBe(false)
  })
})
