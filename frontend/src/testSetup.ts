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

// jsdom lays nothing out, so it leaves `elementFromPoint` unimplemented. `input-otp` (HeroUI's
// InputOTP, our AuthenticatorCodeInput) calls it from timers 0, 2, and 5 seconds after the field
// is focused, hit-testing the field's right edge for a password manager's badge. Without it each
// timer throws outside any test, and Vitest fails the run though every test passed. Nothing sits
// at any point of an unlaid page, so the answer is null. `input-otp` reads that as a badge and
// widens the field, which is harmless here. AuthenticatorCodeInput.test.tsx guards it.
document.elementFromPoint = () => null

// Testing Library only unmounts on its own when test globals are enabled; they are not.
afterEach(() => {
  cleanup()
})
