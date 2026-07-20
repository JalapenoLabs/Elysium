// Copyright © 2026 Jalapeno Labs

import type { ProjectView } from './projectsSlice'

// Core
import { describe, expect, it } from 'vitest'
import { create } from '@bufbuild/protobuf'

// Misc
import {
  activeProjectChanged,
  projectsCleared,
  projectsLoaded,
  projectsReducer,
  toProjectView,
} from './projectsSlice'
import { sessionAnonymous } from './sessionSlice'
import { ProjectSchema } from '../proto/generated/project_pb'

const yearloom: ProjectView = { id: 'p1', name: 'Yearloom', slug: 'yearloom', domains: []}
const emailPilot: ProjectView = { id: 'p2', name: 'EmailPilot', slug: 'emailpilot', domains: [ 'emailpilot.io' ]}

function loadedState(preferredProjectId: string | null = null) {
  return projectsReducer(
    undefined,
    projectsLoaded({ projects: [ yearloom, emailPilot ], preferredProjectId }),
  )
}

describe('toProjectView', () => {
  it('projects the generated message to a serializable view', () => {
    const message = create(ProjectSchema, {
      id: { value: 'p9' },
      name: 'NavarroCity',
      slug: 'navarrocity',
      domains: [ 'navarrocity.com' ],
    })

    expect(toProjectView(message)).toEqual({
      id: 'p9',
      name: 'NavarroCity',
      slug: 'navarrocity',
      domains: [ 'navarrocity.com' ],
    })
  })
})

describe('projectsLoaded', () => {
  it('defaults the active project to the first when nothing is restorable', () => {
    const state = loadedState()
    expect(state.activeProjectId).toBe('p1')
    expect(state.isLoaded).toBe(true)
  })

  it('restores the preferred project when it exists in the roster', () => {
    expect(loadedState('p2').activeProjectId).toBe('p2')
  })

  it('ignores a preferred project that is no longer in the roster', () => {
    expect(loadedState('deleted').activeProjectId).toBe('p1')
  })

  it('keeps the current active project across a reload without a preference', () => {
    const switched = projectsReducer(loadedState(), activeProjectChanged('p2'))
    const reloaded = projectsReducer(
      switched,
      projectsLoaded({ projects: [ yearloom, emailPilot ], preferredProjectId: null }),
    )
    expect(reloaded.activeProjectId).toBe('p2')
  })
})

describe('activeProjectChanged', () => {
  it('switches to a known project and rejects an unknown one', () => {
    const state = loadedState()
    expect(projectsReducer(state, activeProjectChanged('p2')).activeProjectId).toBe('p2')
    expect(projectsReducer(state, activeProjectChanged('nope')).activeProjectId).toBe('p1')
  })
})

describe('reset paths', () => {
  it('clears on projectsCleared and on sessionAnonymous', () => {
    const state = loadedState()
    for (const action of [ projectsCleared(), sessionAnonymous() ]) {
      const cleared = projectsReducer(state, action)
      expect(cleared.projects).toEqual([])
      expect(cleared.activeProjectId).toBeNull()
      expect(cleared.isLoaded).toBe(false)
    }
  })
})
