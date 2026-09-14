// Copyright © 2026 Jalapeno Labs

// Core
import { configureStore, createListenerMiddleware } from '@reduxjs/toolkit'

// Redux
import { codingSessionsSlice } from './codingSessionsSlice'
import { llmsSlice } from './llmsSlice'
import { realtimeSlice } from './realtimeSlice'
import { satellitesSlice } from './satellitesSlice'
import { sessionEventsSlice } from './sessionEventsSlice'
import { themeSlice } from './themeSlice'

// The one store for all global state. Server data enters it two ways: thunks that
// fetch a collection, and the event stream (`src/realtime/eventStream.ts`), which
// applies every change the API announces.
export const listenerMiddleware = createListenerMiddleware()

export const store = configureStore({
  reducer: {
    [codingSessionsSlice.name]: codingSessionsSlice.reducer,
    [llmsSlice.name]: llmsSlice.reducer,
    [realtimeSlice.name]: realtimeSlice.reducer,
    [satellitesSlice.name]: satellitesSlice.reducer,
    [sessionEventsSlice.name]: sessionEventsSlice.reducer,
    [themeSlice.name]: themeSlice.reducer,
  },
  middleware: (getDefaultMiddleware) => getDefaultMiddleware()
    .prepend(listenerMiddleware.middleware),
})

export type RootState = ReturnType<typeof store.getState>
export type AppDispatch = typeof store.dispatch
