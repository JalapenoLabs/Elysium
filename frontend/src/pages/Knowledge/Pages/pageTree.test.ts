// Copyright © 2026 Jalapeno Labs

// Core
import { describe, expect, it } from 'vitest'

// Misc
import { makePageSummary } from '../../../testFixtures'
import { buildPageTree, findPageAncestors } from './pageTree'

describe('buildPageTree', () => {
  it('keeps each page under its parent, in the order it arrived', () => {
    const tree = buildPageTree([
      makePageSummary({ id: 'first' }),
      makePageSummary({ id: 'second' }),
      makePageSummary({ id: 'second-child', parentId: 'second' }),
      makePageSummary({ id: 'first-child', parentId: 'first' }),
      makePageSummary({ id: 'first-other-child', parentId: 'first' }),
    ])

    const idsUnder = (parentId: string | null) => tree.childrenByParent
      .get(parentId)
      ?.map((page) => page.id)
    expect(idsUnder(null)).toEqual([ 'first', 'second' ])
    expect(idsUnder('first')).toEqual([ 'first-child', 'first-other-child' ])
    expect(idsUnder('second')).toEqual([ 'second-child' ])
  })

  it('has no entry for a page without children', () => {
    const tree = buildPageTree([ makePageSummary({ id: 'leaf' }) ])

    expect(tree.childrenByParent.has('leaf')).toBe(false)
  })
})

describe('findPageAncestors', () => {
  it('lists the pages above a page, outermost first', () => {
    const tree = buildPageTree([
      makePageSummary({ id: 'top' }),
      makePageSummary({ id: 'middle', parentId: 'top' }),
      makePageSummary({ id: 'leaf', parentId: 'middle' }),
    ])

    expect(findPageAncestors(tree, 'leaf').map((page) => page.id)).toEqual([ 'top', 'middle' ])
  })

  it('has nothing above a page at the top, or one the tree does not hold', () => {
    const tree = buildPageTree([ makePageSummary({ id: 'top' }) ])

    expect(findPageAncestors(tree, 'top')).toEqual([])
    expect(findPageAncestors(tree, 'unknown')).toEqual([])
  })

  it('stops at a parent the tree does not hold', () => {
    const tree = buildPageTree([ makePageSummary({ id: 'leaf', parentId: 'gone' }) ])

    expect(findPageAncestors(tree, 'leaf')).toEqual([])
  })

  it('stops when pages updated apart point back at each other', () => {
    const tree = buildPageTree([
      makePageSummary({ id: 'one', parentId: 'two' }),
      makePageSummary({ id: 'two', parentId: 'one' }),
    ])

    expect(findPageAncestors(tree, 'one').map((page) => page.id)).toEqual([ 'two' ])
  })
})
