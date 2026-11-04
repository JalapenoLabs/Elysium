// Copyright © 2026 Jalapeno Labs

// Core
import { describe, expect, it } from 'vitest'

// Misc
import { getUnknownRouteRedirect } from './unknownRoutes'
import { UNKNOWN_ROUTE_REDIRECT_TO, UrlTree } from './urls'

// Every unknown path once went to Action items, so `/settings/does-not-exist` left Settings
// entirely (#160). Each goes one level up instead, to its nearest parent page.
describe('getUnknownRouteRedirect', () => {
  it('sends an unknown page of a top-level area to that area', () => {
    expect(getUnknownRouteRedirect('/settings/does-not-exist')).toBe(UrlTree.settings)
    expect(getUnknownRouteRedirect('/projects/new/extra')).toBe(UrlTree.projectsNew)
  })

  it('sends an unknown page of a settings page to that settings page', () => {
    expect(getUnknownRouteRedirect('/settings/llms/does-not-exist')).toBe(UrlTree.settingsLlms)
    expect(getUnknownRouteRedirect('/settings/connectors/new/github/extra')).toBe(UrlTree.settingsConnectorNew)
  })

  it('skips a parent that names a record, which may not exist', () => {
    expect(getUnknownRouteRedirect('/studio/item-1/extra')).toBe(UrlTree.studio)
    expect(getUnknownRouteRedirect('/coding/5/extra')).toBe(UrlTree.coding)
    expect(getUnknownRouteRedirect('/action-items/initiatives/initiative-1/extra')).toBe(UrlTree.initiatives)
  })

  // These pages live under `/:id/edit`, so the record's own address matches no route.
  it('sends a record without its edit page to the list it belongs to', () => {
    expect(getUnknownRouteRedirect('/settings/learning-categories/engineering')).toBe(
      UrlTree.settingsLearningCategories,
    )
    expect(getUnknownRouteRedirect('/settings/environment/variable-1')).toBe(UrlTree.settingsEnvironment)
    expect(getUnknownRouteRedirect('/settings/storage/location-1')).toBe(UrlTree.settingsStorage)
    expect(getUnknownRouteRedirect('/settings/llms/llm-1')).toBe(UrlTree.settingsLlms)
  })

  it('climbs as many levels as it takes to reach a page', () => {
    expect(getUnknownRouteRedirect('/settings/a/b/c/d')).toBe(UrlTree.settings)
  })

  it('sends a path with no parent page to the first page in the sidebar', () => {
    expect(getUnknownRouteRedirect('/does-not-exist')).toBe(UNKNOWN_ROUTE_REDIRECT_TO)
    expect(getUnknownRouteRedirect('/does-not/exist/either')).toBe(UNKNOWN_ROUTE_REDIRECT_TO)
  })

  // React Router matches paths regardless of case and trailing slashes, so the parent does too.
  it('finds the parent whatever its case or trailing slash', () => {
    expect(getUnknownRouteRedirect('/Settings/LLMs/nothing/')).toBe(UrlTree.settingsLlms)
  })
})
