// Copyright © 2026 Jalapeno Labs

// Core
import { StrictMode } from 'react'
import { createRoot } from 'react-dom/client'
import { Provider } from 'react-redux'
import { RouterProvider } from 'react-router/dom'

// Redux
import { store } from './store'

// Misc
import './i18n'
import './index.css'
import { startEventStream } from './realtime/eventStream'
import { router } from './router'
import { startThemeSync } from './theme/startThemeSync'

// Both run once, outside React, so StrictMode's double effects never open a second
// event stream or register listeners twice.
startThemeSync()
startEventStream()

const rootElement = document.getElementById('root')
if (!rootElement) {
  throw new Error('index.html is missing the #root element')
}

createRoot(rootElement).render(
  <StrictMode>
    <Provider store={store}>
      <RouterProvider router={router} />
    </Provider>
  </StrictMode>,
)
