// Copyright © 2026 Jalapeno Labs

// Core
import { StrictMode } from 'react'
import { createRoot } from 'react-dom/client'
import { Provider } from 'react-redux'
import { RouterProvider } from 'react-router/dom'
import { SWRConfig } from 'swr'

// Redux
import { store } from './store'

// User interface
import { ConfirmGate } from './gates/ConfirmGate'
import { PromptGate } from './gates/PromptGate'

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

// The event stream refetches on reconnection and pushes every change, so SWR's own
// focus and network revalidation would only repeat requests.
const swrOptions = {
  revalidateOnFocus: false,
  revalidateOnReconnect: false,
}

const rootElement = document.getElementById('root')
if (!rootElement) {
  throw new Error('index.html is missing the #root element')
}

createRoot(rootElement).render(
  <StrictMode>
    <Provider store={store}>
      <SWRConfig value={swrOptions}>
        <ConfirmGate>
          <PromptGate>
            <RouterProvider router={router} />
          </PromptGate>
        </ConfirmGate>
      </SWRConfig>
    </Provider>
  </StrictMode>,
)
