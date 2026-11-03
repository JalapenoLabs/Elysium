// Copyright © 2026 Jalapeno Labs

// Core
import { describe, expect, it } from 'vitest'

// Misc
import { MEETING_STATES } from '../../../api/routes/meetingRoutes'
import { formatDuration, meetingStateChipColors, meetingStateLabelKeys } from './meetingsPresentation'

describe('formatDuration', () => {
  it('reads like a player, with hours only once there are some', () => {
    expect(formatDuration(0)).toBe('0:00')
    expect(formatDuration(65_400)).toBe('1:05')
    expect(formatDuration(3_725_000)).toBe('1:02:05')
  })

  it('answers nothing for a length the source did not give', () => {
    expect(formatDuration(null)).toBeNull()
    expect(formatDuration(-1)).toBeNull()
  })
})

describe('meeting states', () => {
  it('has a label and a color for every state the API sends', () => {
    for (const state of MEETING_STATES) {
      expect(meetingStateLabelKeys[state]).toBe(`states.${state}`)
      expect(meetingStateChipColors[state]).toBeTruthy()
    }
    expect(meetingStateChipColors.failed).toBe('danger')
    expect(meetingStateChipColors.done).toBe('success')
  })
})
