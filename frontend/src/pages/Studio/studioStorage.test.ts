// Copyright © 2026 Jalapeno Labs

// Core
import { describe, expect, it } from 'vitest'

// Misc
import { makeStorageLocation } from '../../testFixtures'
import { getDefaultStudioLocationId, getStudioLocationChoices } from './studioStorage'

const everyProject = makeStorageLocation({ id: 'every', projects: '*' })
const bananaOnly = makeStorageLocation({ id: 'banana-only', projects: [ 'banana' ]})
const noProjects = makeStorageLocation({ id: 'none', projects: []})
const locations = [ everyProject, bananaOnly, noProjects ]

describe('getStudioLocationChoices', () => {
  it('offers only locations for every project when the item has no project', () => {
    const choices = getStudioLocationChoices(locations, null)
    expect(choices.map((location) => location.id)).toEqual([ 'every' ])
  })

  it('adds the locations linked to the chosen project', () => {
    const choices = getStudioLocationChoices(locations, 'banana')
    expect(choices.map((location) => location.id)).toEqual([ 'every', 'banana-only' ])
  })

  it('leaves out locations linked only to other projects', () => {
    const choices = getStudioLocationChoices(locations, 'apple')
    expect(choices.map((location) => location.id)).toEqual([ 'every' ])
  })
})

describe('getDefaultStudioLocationId', () => {
  it('picks the Studio default when it is a choice', () => {
    const studioDefault = makeStorageLocation({ id: 'studio', isStudioDefault: true })
    expect(getDefaultStudioLocationId([ everyProject, studioDefault ])).toBe('studio')
  })

  it('picks a lone choice when no default is offered', () => {
    expect(getDefaultStudioLocationId([ bananaOnly ])).toBe('banana-only')
  })

  it('leaves the choice to the user among several without a default', () => {
    expect(getDefaultStudioLocationId([ everyProject, bananaOnly ])).toBe('')
  })

  it('has nothing to pick from no choices', () => {
    expect(getDefaultStudioLocationId([])).toBe('')
  })
})
