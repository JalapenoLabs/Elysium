// Copyright © 2026 Jalapeno Labs

import type { Mock } from 'vitest'
import type { MeetingCapture } from '../../../api/routes/meetingRoutes'

// Core
import { act, fireEvent, render, screen, waitFor, within } from '@testing-library/react'
import { createMemoryRouter, RouterProvider } from 'react-router'
import { beforeEach, describe, expect, it, vi } from 'vitest'

// User interface
import { MeetingCaptures } from './MeetingCaptures'

// Misc
import { TestAriaRouter } from '../../../testRouter'

type Context = {
  onMoveCapture: Mock<(captureId: string, offsetMs: number) => Promise<boolean>>
  onSplitCapture: Mock<(captureId: string) => Promise<void>>
}

function capture(id: string, overrides: Partial<MeetingCapture> = {}): MeetingCapture {
  return {
    id,
    connectorId: null,
    sourceKind: 'teams',
    url: null,
    offsetMs: 0,
    offsetSource: 'listed',
    hasRecording: true,
    mergedFrom: null,
    ...overrides,
  }
}

const fromFireflies = { meetingId: 'fireflies-meeting', title: 'Launch review' }

function renderCaptures(context: Context, captures: MeetingCapture[]) {
  const router = createMemoryRouter([{
    path: '*',
    element: <TestAriaRouter>
      <MeetingCaptures
        captures={captures}
        onMoveCapture={context.onMoveCapture}
        onSplitCapture={context.onSplitCapture}
      />
    </TestAriaRouter>,
  }])
  render(<RouterProvider router={router} />)
}

// The field a capture's start opens in, once Move is pressed.
function startField() {
  return screen.getByRole('textbox', { name: 'Seconds into the meeting' })
}

// What a person types, committed as the field commits it when focus leaves it.
function type(field: HTMLElement, value: string) {
  fireEvent.change(field, { target: { value }})
  fireEvent.blur(field)
}

describe('MeetingCaptures', () => {
  beforeEach<Context>((context) => {
    context.onMoveCapture = vi.fn(async () => true)
    context.onSplitCapture = vi.fn(async () => {})
  })

  it<Context>('says where each capture starts on the meeting\'s clock, and how that was found', (context) => {
    renderCaptures(context, [
      capture('teams'),
      capture('fireflies', {
        sourceKind: 'fireflies',
        offsetMs: 2_500,
        offsetSource: 'audio',
        mergedFrom: fromFireflies,
      }),
      capture('early', { sourceKind: 'slack', offsetMs: -1_500, offsetSource: 'hand' }),
    ])

    expect(screen.getByText('Starts with the meeting, as its source listed it')).toBeTruthy()
    expect(screen.getByText('Starts 2.5s into the meeting, lined up by its audio')).toBeTruthy()
    expect(screen.getByText('Starts 1.5s before the meeting, set by hand')).toBeTruthy()
  })

  it<Context>('moves a capture by the seconds typed, and closes once it moved', async (context) => {
    renderCaptures(context, [ capture('teams'), capture('fireflies', { sourceKind: 'fireflies', offsetMs: 2_000 }) ])

    const [ , fireflies ] = screen.getAllByRole('listitem')
    fireEvent.click(within(fireflies).getByRole('button', { name: 'Move on the meeting\'s clock' }))
    const field = startField()
    expect((field as HTMLInputElement).value).toBe('2')
    // Nothing changed yet, so there is nothing to apply.
    expect(screen.getByRole('button', { name: 'Move' }).hasAttribute('disabled')).toBe(true)

    type(field, '-1.3')
    fireEvent.click(screen.getByRole('button', { name: 'Move' }))

    await waitFor(() => expect(context.onMoveCapture).toHaveBeenCalledWith('fireflies', -1_300))
    await waitFor(() => expect(screen.queryByRole('textbox', { name: 'Seconds into the meeting' })).toBeNull())
  })

  it<Context>('leaves an offset aligning found between tenths as it is, until the person moves it', async (context) => {
    // Aligning lands on ten milliseconds; the field works in tenths of a second.
    renderCaptures(context, [
      capture('fireflies', { sourceKind: 'fireflies', offsetMs: 2_140, offsetSource: 'audio' }),
    ])

    fireEvent.click(screen.getByRole('button', { name: 'Move on the meeting\'s clock' }))
    const field = startField()
    expect((field as HTMLInputElement).value).toBe('2.1')
    // Focus leaving the field untouched is no move, so nothing turns set by hand by accident.
    fireEvent.blur(field)
    expect(screen.getByRole('button', { name: 'Move' }).hasAttribute('disabled')).toBe(true)

    fireEvent.click(screen.getByRole('button', { name: /Increase/ }))
    fireEvent.click(screen.getByRole('button', { name: 'Move' }))
    await waitFor(() => expect(context.onMoveCapture).toHaveBeenCalledWith('fireflies', 2_200))
  })

  it<Context>('keeps the field open with what was typed when the move is refused', async (context) => {
    context.onMoveCapture.mockResolvedValue(false)
    renderCaptures(context, [ capture('teams') ])

    fireEvent.click(screen.getByRole('button', { name: 'Move on the meeting\'s clock' }))
    type(startField(), '4')
    fireEvent.click(screen.getByRole('button', { name: 'Move' }))

    await waitFor(() => expect(context.onMoveCapture).toHaveBeenCalledWith('teams', 4_000))
    expect((startField() as HTMLInputElement).value).toBe('4')
  })

  it<Context>('leaves a capture where it is on Esc, before the page could go up a level', (context) => {
    renderCaptures(context, [ capture('teams') ])

    fireEvent.click(screen.getByRole('button', { name: 'Move on the meeting\'s clock' }))
    const escape = new KeyboardEvent('keydown', { key: 'Escape', bubbles: true, cancelable: true })
    const field = startField()
    act(() => {
      field.dispatchEvent(escape)
    })

    expect(escape.defaultPrevented).toBe(true)
    expect(screen.queryByRole('textbox', { name: 'Seconds into the meeting' })).toBeNull()
    expect(context.onMoveCapture).not.toHaveBeenCalled()
  })

  it<Context>('boxes what a merge brought under the meeting it came from, and splits it out', async (context) => {
    renderCaptures(context, [
      capture('teams'),
      capture('fireflies-audio', { sourceKind: 'fireflies', mergedFrom: fromFireflies }),
      capture('fireflies-words', { sourceKind: 'fireflies', mergedFrom: fromFireflies }),
    ])

    const merged = screen.getByRole('region', { name: 'From “Launch review”' })
    expect(within(merged).getAllByRole('listitem')).toHaveLength(2)
    expect(within(merged).getByRole('link', { name: 'From “Launch review”' }).getAttribute('href'))
      .toBe('/knowledge/meetings/fireflies-meeting')

    fireEvent.click(within(merged).getByRole('button', { name: 'Split out' }))

    await waitFor(() => expect(context.onSplitCapture).toHaveBeenCalledWith('fireflies-audio'))
    expect(context.onSplitCapture).toHaveBeenCalledTimes(1)
  })

  it<Context>('holds Split out while a capture it would move back was moved by hand since', (context) => {
    renderCaptures(context, [
      capture('teams'),
      capture('fireflies', { sourceKind: 'fireflies', offsetSource: 'hand', mergedFrom: fromFireflies }),
    ])

    const split = screen.getByText('Split out').closest('button')
    if (!split) {
      throw new Error('no Split out button')
    }
    expect(split.hasAttribute('disabled')).toBe(true)
    fireEvent.click(split)
    expect(context.onSplitCapture).not.toHaveBeenCalled()
  })
})
