// Copyright © 2026 Jalapeno Labs

// Core
import { describe, expect, it } from 'vitest'

// Misc
import { makePageSummary } from '../../../testFixtures'
import {
  buildPageTree,
  countPagesUnder,
  findDropPlacement,
  findPageAncestors,
  findPagePlacement,
  findSiblingsBesides,
  isPageUnder,
  listPageDestinations,
} from './pageTree'

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

describe('moving pages', () => {
  // Guide holds Chapter, Appendix, and Index; Chapter holds Section; Notes and Archive sit beside it.
  const tree = buildPageTree([
    makePageSummary({ id: 'guide', title: 'Guide' }),
    makePageSummary({ id: 'notes', title: 'Notes' }),
    makePageSummary({ id: 'archive', title: 'Archive' }),
    makePageSummary({ id: 'chapter', parentId: 'guide', title: 'Chapter' }),
    makePageSummary({ id: 'appendix', parentId: 'guide', title: 'Appendix' }),
    makePageSummary({ id: 'index', parentId: 'guide', title: 'Index' }),
    makePageSummary({ id: 'section', parentId: 'chapter', title: 'Section' }),
  ])

  describe('findPagePlacement', () => {
    it('names a page by its parent and the sibling before it', () => {
      expect(findPagePlacement(tree, makePageSummary({ id: 'appendix', parentId: 'guide' })))
        .toEqual({ parentId: 'guide', after: 'chapter' })
    })

    it('names the first sibling as following nothing', () => {
      expect(findPagePlacement(tree, makePageSummary({ id: 'guide' })))
        .toEqual({ parentId: null, after: null })
    })
  })

  describe('findDropPlacement', () => {
    it('reads a drop on a row as going last under it', () => {
      expect(findDropPlacement(tree, 'notes', { key: 'guide', dropPosition: 'on' }))
        .toEqual({ parentId: 'guide', after: 'index' })
    })

    it('reads a drop on a page with no children as going first under it', () => {
      expect(findDropPlacement(tree, 'notes', { key: 'section', dropPosition: 'on' }))
        .toEqual({ parentId: 'section', after: null })
    })

    it('reads a drop after a row as following it at that row\'s level', () => {
      expect(findDropPlacement(tree, 'section', { key: 'guide', dropPosition: 'after' }))
        .toEqual({ parentId: null, after: 'guide' })
    })

    it('reads a drop before the first row as going first', () => {
      expect(findDropPlacement(tree, 'archive', { key: 'guide', dropPosition: 'before' }))
        .toEqual({ parentId: null, after: null })
    })

    it('leaves the page moving out of who it follows when it moves down among its siblings', () => {
      expect(findDropPlacement(tree, 'chapter', { key: 'index', dropPosition: 'before' }))
        .toEqual({ parentId: 'guide', after: 'appendix' })
      expect(findDropPlacement(tree, 'chapter', { key: 'index', dropPosition: 'after' }))
        .toEqual({ parentId: 'guide', after: 'index' })
    })

    it('leaves the page moving out of who it follows when it moves up among its siblings', () => {
      expect(findDropPlacement(tree, 'index', { key: 'appendix', dropPosition: 'before' }))
        .toEqual({ parentId: 'guide', after: 'chapter' })
    })

    it('leaves a page out of who goes last under its own parent', () => {
      expect(findDropPlacement(tree, 'chapter', { key: 'guide', dropPosition: 'on' }))
        .toEqual({ parentId: 'guide', after: 'index' })
    })

    it('asks for nothing when the page is already there', () => {
      expect(findDropPlacement(tree, 'appendix', { key: 'chapter', dropPosition: 'after' })).toBeNull()
      expect(findDropPlacement(tree, 'appendix', { key: 'index', dropPosition: 'before' })).toBeNull()
      expect(findDropPlacement(tree, 'appendix', { key: 'appendix', dropPosition: 'before' })).toBeNull()
      expect(findDropPlacement(tree, 'appendix', { key: 'appendix', dropPosition: 'after' })).toBeNull()
      expect(findDropPlacement(tree, 'index', { key: 'guide', dropPosition: 'on' })).toBeNull()
    })

    it('refuses to put a page on itself or under anything under it', () => {
      expect(findDropPlacement(tree, 'guide', { key: 'guide', dropPosition: 'on' })).toBeNull()
      expect(findDropPlacement(tree, 'guide', { key: 'section', dropPosition: 'on' })).toBeNull()
      expect(findDropPlacement(tree, 'guide', { key: 'chapter', dropPosition: 'after' })).toBeNull()
      expect(findDropPlacement(tree, 'guide', { key: 'section', dropPosition: 'before' })).toBeNull()
    })

    it('asks for nothing about a page the tree no longer holds', () => {
      expect(findDropPlacement(tree, 'gone', { key: 'guide', dropPosition: 'on' })).toBeNull()
      expect(findDropPlacement(tree, 'notes', { key: 'gone', dropPosition: 'on' })).toBeNull()
    })
  })

  describe('findSiblingsBesides', () => {
    it('lists a parent\'s children without the page moving', () => {
      const ids = findSiblingsBesides(tree, 'guide', 'appendix').map((sibling) => sibling.id)
      expect(ids).toEqual([ 'chapter', 'index' ])
    })

    it('answers nothing under a page with no children', () => {
      expect(findSiblingsBesides(tree, 'section', 'notes')).toEqual([])
    })
  })

  describe('isPageUnder', () => {
    it('sees a page at any depth under another, but not the page itself', () => {
      expect(isPageUnder(tree, 'section', 'guide')).toBe(true)
      expect(isPageUnder(tree, 'guide', 'guide')).toBe(false)
      expect(isPageUnder(tree, 'notes', 'guide')).toBe(false)
    })
  })

  describe('countPagesUnder', () => {
    it('counts every depth, and not the page', () => {
      expect(countPagesUnder(tree, 'guide')).toBe(4)
      expect(countPagesUnder(tree, 'chapter')).toBe(1)
      expect(countPagesUnder(tree, 'notes')).toBe(0)
    })
  })

  describe('listPageDestinations', () => {
    it('offers every page in the tree\'s order, with the titles above it', () => {
      expect(listPageDestinations(tree, 'notes')).toEqual([
        { id: 'guide', title: 'Guide', ancestorTitles: []},
        { id: 'chapter', title: 'Chapter', ancestorTitles: [ 'Guide' ]},
        { id: 'section', title: 'Section', ancestorTitles: [ 'Guide', 'Chapter' ]},
        { id: 'appendix', title: 'Appendix', ancestorTitles: [ 'Guide' ]},
        { id: 'index', title: 'Index', ancestorTitles: [ 'Guide' ]},
        { id: 'archive', title: 'Archive', ancestorTitles: []},
      ])
    })

    it('leaves out the page and everything under it', () => {
      const ids = listPageDestinations(tree, 'chapter').map((destination) => destination.id)
      expect(ids).toEqual([ 'guide', 'appendix', 'index', 'notes', 'archive' ])
    })
  })
})
