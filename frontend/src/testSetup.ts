// Copyright © 2026 Jalapeno Labs

// Core
import { afterEach } from 'vitest'
import { cleanup } from '@testing-library/react'

// Misc
// The same i18next instance the app starts with, so components render real en-US text.
import './i18n'

// jsdom has no media queries, and the theme reads the OS color scheme as the store loads.
// Every query answers as unmatched, which resolves the System theme to light.
window.matchMedia = (query: string) => ({
  matches: false,
  media: query,
  onchange: null,
  addEventListener: () => {},
  removeEventListener: () => {},
  addListener: () => {},
  removeListener: () => {},
  dispatchEvent: () => false,
})

// Testing Library only unmounts on its own when test globals are enabled; they are not.
afterEach(() => {
  cleanup()
})
