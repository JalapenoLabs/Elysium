// Copyright © 2026 Jalapeno Labs

// Core
import { configureStore, createListenerMiddleware } from '@reduxjs/toolkit'

// Redux
import { actionItemCommentsSlice } from './actionItemCommentsSlice'
import { actionItemHistorySlice } from './actionItemHistorySlice'
import { actionItemLinksSlice } from './actionItemLinksSlice'
import { actionItemsSlice } from './actionItemsSlice'
import { changesetsSlice } from './changesetsSlice'
import { codingSessionsSlice } from './codingSessionsSlice'
import { environmentVariablesSlice } from './environmentVariablesSlice'
import { githubCredentialsSlice } from './githubCredentialsSlice'
import { initiativeLinksSlice } from './initiativeLinksSlice'
import { initiativesSlice } from './initiativesSlice'
import { jiraCredentialsSlice } from './jiraCredentialsSlice'
import { llmsSlice } from './llmsSlice'
import { mailAccountsSlice } from './mailAccountsSlice'
import { mailDomainsSlice } from './mailDomainsSlice'
import { mailServerSlice } from './mailServerSlice'
import { projectsSlice } from './projectsSlice'
import { realtimeSlice } from './realtimeSlice'
import { satellitesSlice } from './satellitesSlice'
import { sessionEventsSlice } from './sessionEventsSlice'
import { storageLocationsSlice } from './storageLocationsSlice'
import { themeSlice } from './themeSlice'

// The one store for all global state, and the source of truth components render. Server
// data enters it two ways: SWR loaders that fetch it once (`src/hooks/useServerData.ts`),
// and the event stream (`src/realtime/eventStream.ts`), which applies every change the
// API announces.
export const listenerMiddleware = createListenerMiddleware()

// Builds a store with every slice. The app holds one (`store` below); tests build their own,
// so each starts from empty state.
export function createAppStore() {
  return configureStore({
    reducer: {
      [actionItemCommentsSlice.name]: actionItemCommentsSlice.reducer,
      [actionItemHistorySlice.name]: actionItemHistorySlice.reducer,
      [actionItemLinksSlice.name]: actionItemLinksSlice.reducer,
      [actionItemsSlice.name]: actionItemsSlice.reducer,
      [changesetsSlice.name]: changesetsSlice.reducer,
      [codingSessionsSlice.name]: codingSessionsSlice.reducer,
      [environmentVariablesSlice.name]: environmentVariablesSlice.reducer,
      [githubCredentialsSlice.name]: githubCredentialsSlice.reducer,
      [initiativeLinksSlice.name]: initiativeLinksSlice.reducer,
      [initiativesSlice.name]: initiativesSlice.reducer,
      [jiraCredentialsSlice.name]: jiraCredentialsSlice.reducer,
      [llmsSlice.name]: llmsSlice.reducer,
      [mailAccountsSlice.name]: mailAccountsSlice.reducer,
      [mailDomainsSlice.name]: mailDomainsSlice.reducer,
      [mailServerSlice.name]: mailServerSlice.reducer,
      [projectsSlice.name]: projectsSlice.reducer,
      [realtimeSlice.name]: realtimeSlice.reducer,
      [satellitesSlice.name]: satellitesSlice.reducer,
      [sessionEventsSlice.name]: sessionEventsSlice.reducer,
      [storageLocationsSlice.name]: storageLocationsSlice.reducer,
      [themeSlice.name]: themeSlice.reducer,
    },
    middleware: (getDefaultMiddleware) => getDefaultMiddleware()
      .prepend(listenerMiddleware.middleware),
  })
}

export const store = createAppStore()

export type RootState = ReturnType<typeof store.getState>
export type AppDispatch = typeof store.dispatch
