// Copyright © 2026 Jalapeno Labs

// Urls
export const UrlTree = {
  root: '/',
  settings: '/settings',
  settingsPersonalDetails: '/settings/personal-details',
  settingsLlms: '/settings/llms',
} as const
export type UrlValue = typeof UrlTree[keyof typeof UrlTree]

// Settings
export const UNKNOWN_ROUTE_REDIRECT_TO: UrlValue = UrlTree.root
