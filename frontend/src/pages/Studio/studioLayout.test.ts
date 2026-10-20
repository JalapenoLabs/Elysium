// Copyright © 2026 Jalapeno Labs

// Core
import { describe, expect, it } from 'vitest'

// Misc
import {
  CHAT_PANEL_ID,
  DEFAULT_STUDIO_LAYOUT,
  getLayoutAfterResize,
  getPanelLayout,
  parseStoredStudioLayout,
  PREVIEW_PANEL_ID,
} from './studioLayout'

describe('parseStoredStudioLayout', () => {
  it('uses the default when nothing is stored', () => {
    expect(parseStoredStudioLayout(null)).toEqual(DEFAULT_STUDIO_LAYOUT)
  })

  it('reads a stored layout', () => {
    const stored = JSON.stringify({ mode: 'chat', previewPercent: 45 })
    expect(parseStoredStudioLayout(stored)).toEqual({ mode: 'chat', previewPercent: 45 })
  })

  it('uses the default for a value that is not JSON or not an object', () => {
    expect(parseStoredStudioLayout('{nope')).toEqual(DEFAULT_STUDIO_LAYOUT)
    expect(parseStoredStudioLayout('42')).toEqual(DEFAULT_STUDIO_LAYOUT)
    expect(parseStoredStudioLayout('null')).toEqual(DEFAULT_STUDIO_LAYOUT)
  })

  it('replaces an unknown mode and an out-of-range split one field at a time', () => {
    const stored = JSON.stringify({ mode: 'neither', previewPercent: 95 })
    expect(parseStoredStudioLayout(stored)).toEqual(DEFAULT_STUDIO_LAYOUT)

    const partial = JSON.stringify({ mode: 'preview', previewPercent: 'wide' })
    expect(parseStoredStudioLayout(partial)).toEqual({
      mode: 'preview',
      previewPercent: DEFAULT_STUDIO_LAYOUT.previewPercent,
    })
  })
})

describe('getPanelLayout', () => {
  it('splits the width while both columns show', () => {
    expect(getPanelLayout({ mode: 'both', previewPercent: 65 })).toEqual({
      [PREVIEW_PANEL_ID]: 65,
      [CHAT_PANEL_ID]: 35,
    })
  })

  it('gives one column the whole width otherwise, whatever the split', () => {
    expect(getPanelLayout({ mode: 'preview', previewPercent: 65 })).toEqual({
      [PREVIEW_PANEL_ID]: 100,
      [CHAT_PANEL_ID]: 0,
    })
    expect(getPanelLayout({ mode: 'chat', previewPercent: 65 })).toEqual({
      [PREVIEW_PANEL_ID]: 0,
      [CHAT_PANEL_ID]: 100,
    })
  })
})

describe('getLayoutAfterResize', () => {
  const previous = { mode: 'both', previewPercent: 60 } as const

  it('keeps both columns at the new split', () => {
    const panels = { [PREVIEW_PANEL_ID]: 42.5, [CHAT_PANEL_ID]: 57.5 }
    expect(getLayoutAfterResize(panels, previous)).toEqual({ mode: 'both', previewPercent: 42.5 })
  })

  it('switches to Chat when the stage is dragged closed, remembering the split', () => {
    const panels = { [PREVIEW_PANEL_ID]: 0, [CHAT_PANEL_ID]: 100 }
    expect(getLayoutAfterResize(panels, previous)).toEqual({ mode: 'chat', previewPercent: 60 })
  })

  it('switches to Preview when the conversation is dragged closed', () => {
    const panels = { [PREVIEW_PANEL_ID]: 99.8, [CHAT_PANEL_ID]: 0.2 }
    expect(getLayoutAfterResize(panels, previous)).toEqual({ mode: 'preview', previewPercent: 60 })
  })

  it('shows both again when a hidden column is dragged open', () => {
    const fromChat = { mode: 'chat', previewPercent: 60 } as const
    const panels = { [PREVIEW_PANEL_ID]: 30, [CHAT_PANEL_ID]: 70 }
    expect(getLayoutAfterResize(panels, fromChat)).toEqual({ mode: 'both', previewPercent: 30 })
  })

  it('keeps the layout when the change names no stage', () => {
    expect(getLayoutAfterResize({ other: 100 }, previous)).toBe(previous)
  })
})
