// Copyright © 2026 Jalapeno Labs

// Single source of truth for in-app navigation targets. All navigation resolves
// through here rather than hardcoding paths across the app.
export const UrlTree = {
  home: '/',
  about: '/about',

  // Sign-in ceremony (the credential steps themselves are Keycloak's themed flow;
  // these are the app-side entry and exit points).
  signIn: '/sign-in',
  signedOut: '/signed-out',

  // Bootstrap & onboarding.
  setup: '/setup',
  onboarding: '/onboarding',
  onboardingProfile: '/onboarding/profile',

  // Invitations (signed link).
  invite: '/invite/:token',

  // Account & security.
  settingsSecurity: '/settings/security',
  settingsSessions: '/settings/sessions',
  settingsProfile: '/settings/profile',

  // Denied & fault states.
  accessDenied: '/denied',
  suspended: '/suspended',
  identityUnavailable: '/identity-unavailable',
} as const

export type UrlValue = typeof UrlTree[keyof typeof UrlTree]

export const ExternalLinks = {
  heroui: 'https://www.heroui.com',
  vite: 'https://vite.dev',
  reactRouter: 'https://reactrouter.com',
} as const

export type ExternalLinkValue = typeof ExternalLinks[keyof typeof ExternalLinks]

// Builds the acceptance URL for a signed invite token.
export const getInviteUrl = (token: string) =>
  UrlTree.invite.replace(':token', token)
