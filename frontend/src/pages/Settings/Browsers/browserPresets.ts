// Copyright © 2026 Jalapeno Labs

// The services a browser is usually added for, each with the page to sign in at. `other`
// leaves the page to the person.
export const BROWSER_PRESETS = [ 'outlook', 'gmail', 'other' ] as const
export type BrowserPreset = typeof BROWSER_PRESETS[number]

export const startUrlByPreset = {
  outlook: 'https://outlook.office.com/mail/',
  gmail: 'https://mail.google.com/mail/',
  other: '',
} as const satisfies Record<BrowserPreset, string>
