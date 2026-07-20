// Copyright © 2026 Jalapeno Labs

// Core
import { createRoot } from 'react-dom/client'
import { Provider } from 'react-redux'
import { RouterProvider } from 'react-router'

// Redux
import { store } from './store'

// Misc
import { router } from './router'
import { i18nReady } from './i18n'

import './index.css'

const container = document.getElementById('root')
if (!container) {
  throw new Error('Root element #root was not found in index.html')
}
const root = createRoot(container)

// Await the initial locale bundles (fetched at runtime from /public/locales) so
// the first paint already has translations. The session itself resolves later:
// it lives in an HttpOnly cookie the browser can't read, so RootLayout probes the
// API and the route guards react to the result.
//
// StrictMode is intentionally off: its dev-only double-invocation of effects
// double-fired our load-time fetches (session probe, page data, TOTP begin),
// which was noise during development. Production was never affected.
i18nReady
  .catch((error) => {
    console.debug('i18n failed to load initial locale; rendering with keys', error)
  })
  .finally(() => {
    root.render(
      <Provider store={store}>
        <RouterProvider router={router} />
      </Provider>,
    )
  })
