// Copyright © 2026 Jalapeno Labs

// Core
import { StrictMode } from 'react'
import { createRoot } from 'react-dom/client'
import { Provider } from 'react-redux'
import { RouterProvider } from 'react-router'

// Redux
import { store } from './store'

// Misc
import { initKeycloak } from './auth/keycloak'
import { router } from './router'

import './index.css'

const container = document.getElementById('root')
if (!container) {
  throw new Error('Root element #root was not found in index.html')
}
const root = createRoot(container)

// Resolve the Keycloak session before the first render so the UI knows whether
// the user is authenticated. If Keycloak is unreachable, render anonymously
// rather than blocking the whole app on the identity provider.
async function bootstrap() {
  try {
    await initKeycloak()
  }
  catch (error) {
    console.debug('Keycloak init failed; continuing unauthenticated', error)
  }

  root.render(
    <StrictMode>
      <Provider store={store}>
        <RouterProvider router={router} />
      </Provider>
    </StrictMode>,
  )
}

bootstrap()
