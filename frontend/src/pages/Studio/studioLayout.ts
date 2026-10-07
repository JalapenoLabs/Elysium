// Copyright © 2026 Jalapeno Labs

import type { Layout } from 'react-resizable-panels'

// The item page's two columns, the stage and the conversation, and which of them show.
// One mode at a time, so the two can never both be hidden.
export const STUDIO_LAYOUT_MODES = [ 'both', 'preview', 'chat' ] as const
export type StudioLayoutMode = typeof STUDIO_LAYOUT_MODES[number]

export type StudioLayout = {
  mode: StudioLayoutMode
  // The stage's share of the width, in percent, while both columns show. Kept while one
  // column is hidden, so showing both again restores the split.
  previewPercent: number
}

// The panel ids the split is keyed by.
export const PREVIEW_PANEL_ID = 'preview'
export const CHAT_PANEL_ID = 'chat'

// Neither column may be narrowed past this share while both show; dragging past it hides
// the column instead.
export const MIN_PANEL_PERCENT = 20

export const DEFAULT_STUDIO_LAYOUT: StudioLayout = {
  mode: 'both',
  previewPercent: 60,
}

// Anything unreadable, unknown, or out of range falls back to the default, so a corrupt or
// foreign value never breaks the page.
export function parseStoredStudioLayout(stored: string | null): StudioLayout {
  if (!stored) {
    return DEFAULT_STUDIO_LAYOUT
  }

  let parsed: unknown
  try {
    parsed = JSON.parse(stored)
  }
  catch (error) {
    console.debug('The stored Studio layout is not JSON; using the default', { error })
    return DEFAULT_STUDIO_LAYOUT
  }
  if (typeof parsed !== 'object' || parsed === null) {
    console.debug('The stored Studio layout is not an object; using the default', { parsed })
    return DEFAULT_STUDIO_LAYOUT
  }

  const mode = 'mode' in parsed
    ? STUDIO_LAYOUT_MODES.find((candidate) => candidate === parsed.mode)
    : undefined
  const previewPercent = 'previewPercent' in parsed
    ? parsed.previewPercent
    : undefined
  const isPercentValid = typeof previewPercent === 'number'
    && previewPercent >= MIN_PANEL_PERCENT
    && previewPercent <= 100 - MIN_PANEL_PERCENT

  return {
    mode: mode ?? DEFAULT_STUDIO_LAYOUT.mode,
    previewPercent: isPercentValid
      ? previewPercent
      : DEFAULT_STUDIO_LAYOUT.previewPercent,
  }
}

// The panel sizes that show a layout: the split when both show, else the one column whole.
export function getPanelLayout(layout: StudioLayout): Layout {
  const previewPercentByMode = {
    both: layout.previewPercent,
    preview: 100,
    chat: 0,
  } as const satisfies Record<StudioLayoutMode, number>

  const previewPercent = previewPercentByMode[layout.mode]
  return {
    [PREVIEW_PANEL_ID]: previewPercent,
    [CHAT_PANEL_ID]: 100 - previewPercent,
  }
}

// Within this many percent of zero, a column counts as hidden: the panel library collapses
// to exactly zero, and the margin absorbs rounding.
const COLLAPSED_PERCENT = 0.5

// The layout after the user dragged the split. A column dragged past its collapse point
// arrives at zero, which switches to showing only the other; otherwise both show at the new
// split.
export function getLayoutAfterResize(panels: Layout, previous: StudioLayout): StudioLayout {
  const previewPercent = panels[PREVIEW_PANEL_ID]
  if (previewPercent === undefined) {
    console.debug('A Studio layout change named no preview panel; keeping the layout', { panels })
    return previous
  }

  if (previewPercent <= COLLAPSED_PERCENT) {
    return { ...previous, mode: 'chat' }
  }
  if (previewPercent >= 100 - COLLAPSED_PERCENT) {
    return { ...previous, mode: 'preview' }
  }
  return { mode: 'both', previewPercent }
}
