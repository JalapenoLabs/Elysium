// Copyright © 2026 Jalapeno Labs

// Core
import { StrictMode } from 'react'
import { createRoot } from 'react-dom/client'
import { RouterProvider } from 'react-router/dom'

// Misc
import './i18n'
import './index.css'
import { router } from './router'
import { initializeTheme } from './theme/themePreference'

initializeTheme()

const rootElement = document.getElementById('root')
if (!rootElement) {
  throw new Error('index.html is missing the #root element')
}

createRoot(rootElement).render(
  <StrictMode>
    <RouterProvider router={router} />
  </StrictMode>,
)
