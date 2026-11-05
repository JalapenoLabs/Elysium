// Copyright © 2026 Jalapeno Labs

import type { ReactNode } from 'react'

// Core
import { act, renderHook } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { Provider } from 'react-redux'

// Redux
import { createAppStore } from '../../../store'
import { meLoaded } from '../../../store/authSlice'

// Misc
import { PAGE_TREE_STORAGE_KEY } from '../../../constants'
import { makeUser } from '../../../testFixtures'
import { readPageTreeMemory, usePageTreeMemory } from './usePageTreeMemory'

type WrapperProps = {
  children: ReactNode
}

const storageKeyOf = (personId: string) => `${PAGE_TREE_STORAGE_KEY}.${personId}`

function renderMemory(personId: string) {
  const store = createAppStore()
  store.dispatch(meLoaded({
    user: makeUser({ id: personId }),
    hasSecondFactor: false,
    mfaEnrollmentRequired: false,
    authenticatorNickname: null,
  }))

  function Wrapper(props: WrapperProps) {
    return <Provider store={store}>{props.children}</Provider>
  }

  return renderHook(() => usePageTreeMemory(), { wrapper: Wrapper })
}

describe('readPageTreeMemory', () => {
  beforeEach(() => {
    window.localStorage.clear()
    vi.spyOn(console, 'debug').mockImplementation(() => undefined)
  })

  it('starts with everything closed', () => {
    expect(readPageTreeMemory('nothing-stored')).toEqual({ openIds: [], isCollapsed: false })
  })

  it('starts over from what it cannot read', () => {
    window.localStorage.setItem('broken', '{ not json')
    window.localStorage.setItem('wrong-shape', '{"openIds":"all"}')

    expect(readPageTreeMemory('broken').openIds).toEqual([])
    expect(readPageTreeMemory('wrong-shape').openIds).toEqual([])
  })
})

describe('usePageTreeMemory', () => {
  beforeEach(() => {
    window.localStorage.clear()
  })

  it('keeps open branches for the same person', () => {
    const first = renderMemory('person')
    act(() => first.result.current.setOpenIds(new Set([ 'one', 'two' ])))
    first.unmount()

    const second = renderMemory('person')

    expect([ ...second.result.current.openIds ]).toEqual([ 'one', 'two' ])
  })

  it('keeps each person\'s branches apart on one browser', () => {
    const first = renderMemory('person')
    act(() => first.result.current.setOpenIds(new Set([ 'one' ])))
    first.unmount()

    const other = renderMemory('someone-else')

    expect(other.result.current.openIds.size).toBe(0)
  })

  it('opens branches without closing the others, and leaves a repeat alone', () => {
    const { result } = renderMemory('person')
    act(() => result.current.setOpenIds(new Set([ 'one' ])))

    act(() => result.current.openBranches([ 'one', 'two' ]))

    expect([ ...result.current.openIds ]).toEqual([ 'one', 'two' ])

    const before = result.current.openIds
    act(() => result.current.openBranches([ 'two' ]))
    expect(result.current.openIds).toBe(before)
  })

  it('remembers the panel collapsed', () => {
    const first = renderMemory('person')
    act(() => first.result.current.setIsCollapsed(true))
    first.unmount()

    expect(renderMemory('person').result.current.isCollapsed).toBe(true)
  })

  it('follows a change saved in another tab', () => {
    const { result } = renderMemory('person')
    const storageKey = storageKeyOf('person')

    act(() => {
      window.localStorage.setItem(storageKey, JSON.stringify({ openIds: [ 'theirs' ], isCollapsed: false }))
      window.dispatchEvent(new StorageEvent('storage', { key: storageKey }))
    })

    expect([ ...result.current.openIds ]).toEqual([ 'theirs' ])
  })
})
