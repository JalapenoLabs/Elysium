// Copyright © 2026 Jalapeno Labs

import type { PayloadAction } from '@reduxjs/toolkit'
import type { Elysia, ElysiaMessage, ElysiaTurn } from '../api/routes/elysiaRoutes'
import type { RootState } from './index'

// Core
import { createEntityAdapter, createSlice } from '@reduxjs/toolkit'

// Misc
import { ELYSIA_PANEL_STORAGE_KEY } from '../constants'

// Message and turn ids are UUIDv7, so comparing them as text orders them by time.
const messagesAdapter = createEntityAdapter<ElysiaMessage>({
  sortComparer: (first, second) => first.id.localeCompare(second.id),
})

const turnsAdapter = createEntityAdapter<ElysiaTurn>({
  sortComparer: (first, second) => second.id.localeCompare(first.id),
})

type ElysiaSliceState = {
  // Null until the first load. Replaced whole by every load and `elysia.updated` event.
  elysia: Elysia | null
  // The conversation, oldest first.
  messages: ReturnType<typeof messagesAdapter.getInitialState>
  // Her latest turns, newest first.
  turns: ReturnType<typeof turnsAdapter.getInitialState>
  // Whether her panel is open beside the page, remembered per browser.
  isPanelOpen: boolean
}

// Storage can be unavailable (a private window, blocked site data); the panel then starts
// closed rather than failing to load.
function readStoredPanelOpen() {
  try {
    return window.localStorage.getItem(ELYSIA_PANEL_STORAGE_KEY) === 'open'
  }
  catch (error) {
    console.debug('Could not read whether the Elysia panel was open', { error })
    return false
  }
}

const initialState: ElysiaSliceState = {
  elysia: null,
  messages: messagesAdapter.getInitialState(),
  turns: turnsAdapter.getInitialState(),
  isPanelOpen: readStoredPanelOpen(),
}

export const elysiaSlice = createSlice({
  name: 'elysia',
  initialState,
  reducers: {
    elysiaUpdated(state, action: PayloadAction<Elysia>) {
      state.elysia = action.payload
    },
    // A page of the conversation arrived; pages merge, since the stream may already hold
    // messages newer than the page.
    elysiaMessagesLoaded(state, action: PayloadAction<ElysiaMessage[]>) {
      messagesAdapter.upsertMany(state.messages, action.payload)
    },
    elysiaMessageCreated(state, action: PayloadAction<ElysiaMessage>) {
      messagesAdapter.upsertOne(state.messages, action.payload)
    },
    elysiaTurnsLoaded(state, action: PayloadAction<ElysiaTurn[]>) {
      turnsAdapter.upsertMany(state.turns, action.payload)
    },
    elysiaTurnUpserted(state, action: PayloadAction<ElysiaTurn>) {
      turnsAdapter.upsertOne(state.turns, action.payload)
    },
    elysiaPanelToggled(state, action: PayloadAction<boolean | undefined>) {
      state.isPanelOpen = action.payload ?? !state.isPanelOpen
    },
  },
})

export const {
  elysiaUpdated,
  elysiaMessagesLoaded,
  elysiaMessageCreated,
  elysiaTurnsLoaded,
  elysiaTurnUpserted,
  elysiaPanelToggled,
} = elysiaSlice.actions

export const selectElysia = (state: RootState) => state.elysia.elysia
export const selectIsElysiaPanelOpen = (state: RootState) => state.elysia.isPanelOpen

export const {
  selectAll: selectElysiaMessages,
} = messagesAdapter.getSelectors((state: RootState) => state.elysia.messages)

export const {
  selectAll: selectElysiaTurns,
} = turnsAdapter.getSelectors((state: RootState) => state.elysia.turns)
