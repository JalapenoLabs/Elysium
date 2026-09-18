// Copyright © 2026 Jalapeno Labs

// Core
import { describe, expect, it } from 'vitest'

// Misc
import { makeActionItem, makeProject } from '../../testFixtures'
import { getSessionProjectChoices } from './sessionItemProjects'

const PROJECTS = [
  makeProject({ id: 'elysium' }),
  makeProject({ id: 'farworlds' }),
  makeProject({ id: 'uikit' }),
]

function ids(projects: { id: string }[]) {
  return projects.map((project) => project.id)
}

describe('getSessionProjectChoices', () => {
  it('offers every project to a session of its own', () => {
    expect(ids(getSessionProjectChoices(null, PROJECTS))).toEqual([ 'elysium', 'farworlds', 'uikit' ])
  })

  it('offers every project when the item is in none', () => {
    const item = makeActionItem({ id: 'item' })
    expect(ids(getSessionProjectChoices(item, PROJECTS))).toEqual([ 'elysium', 'farworlds', 'uikit' ])
  })

  it('offers only the item\'s projects, in the order projects are listed', () => {
    const item = makeActionItem({ id: 'item', projectIds: [ 'uikit', 'elysium' ]})
    expect(ids(getSessionProjectChoices(item, PROJECTS))).toEqual([ 'elysium', 'uikit' ])
  })

  it('offers exactly the one project of an item in one', () => {
    const item = makeActionItem({ id: 'item', projectIds: [ 'farworlds' ]})
    expect(ids(getSessionProjectChoices(item, PROJECTS))).toEqual([ 'farworlds' ])
  })
})
