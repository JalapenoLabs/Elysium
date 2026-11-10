// Copyright © 2026 Jalapeno Labs

// Core
import { describe, expect, it } from 'vitest'

// Misc
import { MEETINGS_PAGE_ID } from '../../../constants'
import { makeMeeting, makePageSummary } from '../../../testFixtures'
import {
  buildPageTree,
  collectLabelsInUse,
  countPagesUnder,
  filterPageTree,
  holdsSamePages,
  findDropPlacement,
  findPageAncestors,
  findPagePlacement,
  findSiblingsBesides,
  isPageUnder,
  listPageDestinations,
  withMeetingEntries,
} from './pageTree'

describe('withMeetingEntries', () => {
  const pages = buildPageTree([
    makePageSummary({ id: MEETINGS_PAGE_ID, title: 'Meetings', fixed: true }),
    makePageSummary({ id: 'briefs', title: 'Project briefs', fixed: true }),
    makePageSummary({ id: 'launch-brief', parentId: 'briefs', title: 'Launch brief', briefProjectId: 'launch' }),
    makePageSummary({ id: 'billing-brief', parentId: 'briefs', title: 'Billing brief', briefProjectId: 'billing' }),
  ])
  // Newest first, as the meetings slice holds them.
  const meetings = [
    makeMeeting({ id: 'standup', title: 'Standup', projectIds: [ 'launch', 'billing' ]}),
    makeMeeting({ id: 'one-to-one', title: 'One to one', projectIds: []}),
    makeMeeting({ id: 'retro', title: 'Retro', projectIds: [ 'launch' ]}),
  ]

  it('lists every meeting at its home under Meetings, on a project or not, in their order', () => {
    const tree = withMeetingEntries(pages, meetings)

    const homes = tree.meetingsByPage.get(MEETINGS_PAGE_ID) ?? []
    expect(homes.map((entry) => entry.meetingId)).toEqual([ 'standup', 'one-to-one', 'retro' ])
  })

  it('groups each project\'s meetings under its brief, a meeting on two projects in both', () => {
    const tree = withMeetingEntries(pages, meetings)

    const launch = tree.meetingGroupsByPage.get('launch-brief')
    expect(launch?.projectId).toBe('launch')
    expect(launch?.entries.map((entry) => entry.meetingId)).toEqual([ 'standup', 'retro' ])
    const billing = tree.meetingGroupsByPage.get('billing-brief')
    expect(billing?.entries.map((entry) => entry.meetingId)).toEqual([ 'standup' ])
    // The same meeting is several rows of one tree, so no two share a key.
    const keys = [
      ...(tree.meetingsByPage.get(MEETINGS_PAGE_ID) ?? []),
      ...(launch?.entries ?? []),
      ...(billing?.entries ?? []),
    ].map((entry) => entry.key)
    expect(new Set(keys).size).toBe(keys.length)
  })

  it('gives a project with no brief in the tree no group, and a project with no meeting none', () => {
    const tree = withMeetingEntries(pages, [ makeMeeting({ id: 'elsewhere', projectIds: [ 'deleted' ]}) ])

    expect([ ...tree.meetingGroupsByPage.keys() ]).toEqual([])
  })

  it('lists no meeting while the tree has no Meetings page to hold them', () => {
    const tree = withMeetingEntries(buildPageTree([]), meetings)

    expect(tree.meetingsByPage.size).toBe(0)
  })

  it('keeps the pages as they are, and a filter narrows pages alone', () => {
    const tree = withMeetingEntries(pages, meetings)
    expect(tree.byId).toBe(pages.byId)

    const filtered = filterPageTree(tree, { label: null, title: 'launch' })
    expect([ ...filtered.byId.keys() ]).toEqual([ 'briefs', 'launch-brief' ])
    expect(filtered.meetingsByPage.size).toBe(0)
    expect(filtered.meetingGroupsByPage.size).toBe(0)
  })
})

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

describe('filterPageTree', () => {
  const summaries = [
    makePageSummary({ id: 'guide' }),
    makePageSummary({ id: 'chapter', parentId: 'guide' }),
    makePageSummary({ id: 'runbook', parentId: 'chapter', labels: [ 'on-call' ]}),
    makePageSummary({ id: 'other-chapter', parentId: 'guide' }),
    makePageSummary({ id: 'alerts', labels: [ 'on-call', 'pager' ]}),
    makePageSummary({ id: 'alert-rules', parentId: 'alerts' }),
    makePageSummary({ id: 'notes' }),
  ]
  const tree = buildPageTree(summaries)

  it('is the tree itself with nothing to filter by', () => {
    expect(filterPageTree(tree, { label: null, title: '' })).toBe(tree)
  })

  it('keeps each page carrying the label under the pages above it, in order', () => {
    const filtered = filterPageTree(tree, { label: 'on-call', title: '' })

    expect([ ...filtered.byId.keys() ]).toEqual([ 'guide', 'chapter', 'runbook', 'alerts' ])
    expect(filtered.childrenByParent.get(null)?.map((page) => page.id)).toEqual([ 'guide', 'alerts' ])
    expect(filtered.childrenByParent.get('guide')?.map((page) => page.id)).toEqual([ 'chapter' ])
    expect([ ...filtered.contextIds ?? [] ]).toEqual([ 'chapter', 'guide' ])
  })

  it('counts a page as a match however its children came first', () => {
    const childFirst = buildPageTree([
      makePageSummary({ id: 'child', parentId: 'parent', labels: [ 'on-call' ]}),
      makePageSummary({ id: 'parent', labels: [ 'on-call' ]}),
    ])

    expect(filterPageTree(childFirst, { label: 'on-call', title: '' }).contextIds?.size).toBe(0)
  })

  it('leaves out what sits under a match without carrying it', () => {
    const filtered = filterPageTree(tree, { label: 'pager', title: '' })

    expect([ ...filtered.byId.keys() ]).toEqual([ 'alerts' ])
    expect(filtered.childrenByParent.has('alerts')).toBe(false)
  })

  it('keeps the titles typed, under the pages above them, every word beginning a word', () => {
    const titled = buildPageTree([
      makePageSummary({ id: 'guide', title: 'Guide' }),
      makePageSummary({ id: 'chapter', parentId: 'guide', title: 'Operations' }),
      makePageSummary({ id: 'runbook', parentId: 'chapter', title: 'Déploiement runbook' }),
      makePageSummary({ id: 'notes', title: 'Notes' }),
    ])

    const filtered = filterPageTree(titled, { label: null, title: 'run DEPL' })

    expect([ ...filtered.byId.keys() ]).toEqual([ 'guide', 'chapter', 'runbook' ])
    expect([ ...filtered.contextIds ?? [] ]).toEqual([ 'chapter', 'guide' ])
    expect(filterPageTree(titled, { label: null, title: 'book' }).byId.size).toBe(0)
  })

  it('is the tree itself for a title holding no words', () => {
    expect(filterPageTree(tree, { label: null, title: ' - ' })).toBe(tree)
  })

  it('takes a title and a label together', () => {
    const both = buildPageTree([
      makePageSummary({ id: 'kept', title: 'Runbook', labels: [ 'on-call' ]}),
      makePageSummary({ id: 'unlabeled', title: 'Runbook' }),
      makePageSummary({ id: 'other-title', title: 'Notes', labels: [ 'on-call' ]}),
    ])

    expect([ ...filterPageTree(both, { label: 'on-call', title: 'run' }).byId.keys() ]).toEqual([ 'kept' ])
  })

  it('holds nothing when no page carries the label', () => {
    expect(filterPageTree(tree, { label: 'never', title: '' }).byId.size).toBe(0)
  })

  it('keeps a match whose parent the list does not hold, rather than walking past it', () => {
    const orphaned = buildPageTree([ makePageSummary({ id: 'orphan', parentId: 'gone', labels: [ 'on-call' ]}) ])

    expect([ ...filterPageTree(orphaned, { label: 'on-call', title: '' }).byId.keys() ]).toEqual([ 'orphan' ])
  })
})

describe('holdsSamePages', () => {
  const pages = buildPageTree([
    makePageSummary({ id: 'parent', title: 'Guide' }),
    makePageSummary({ id: 'child', parentId: 'parent', title: 'Guide to runbooks' }),
    makePageSummary({ id: 'other', title: 'Notes' }),
  ])

  it('knows that another letter left the same pages showing', () => {
    const first = filterPageTree(pages, { label: null, title: 'run' })
    const second = filterPageTree(pages, { label: null, title: 'runb' })

    expect(holdsSamePages(first, second)).toBe(true)
  })

  it('tells apart a page matched from one kept to show where a match sits', () => {
    const first = filterPageTree(pages, { label: null, title: 'runbooks' })
    const second = filterPageTree(pages, { label: null, title: 'guide' })

    expect([ ...first.byId.keys() ]).toEqual([ ...second.byId.keys() ])
    expect(holdsSamePages(first, second)).toBe(false)
  })

  it('never takes the whole tree for a filter that happens to match all of it', () => {
    const everything = filterPageTree(pages, { label: null, title: 'g n' })

    expect(holdsSamePages(pages, everything)).toBe(false)
  })
})

describe('collectLabelsInUse', () => {
  it('lists every label once, in reading order', () => {
    expect(collectLabelsInUse([
      makePageSummary({ id: 'first', labels: [ 'runbook', 'on-call' ]}),
      makePageSummary({ id: 'second', labels: [ 'on-call', 'billing' ]}),
      makePageSummary({ id: 'third' }),
    ])).toEqual([ 'billing', 'on-call', 'runbook' ])
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

    it('offers everything to a page not yet made', () => {
      expect(listPageDestinations(tree, null)).toHaveLength(tree.byId.size)
    })

    it('leaves out the page and everything under it', () => {
      const ids = listPageDestinations(tree, 'chapter').map((destination) => destination.id)
      expect(ids).toEqual([ 'guide', 'appendix', 'index', 'notes', 'archive' ])
    })
  })
})
