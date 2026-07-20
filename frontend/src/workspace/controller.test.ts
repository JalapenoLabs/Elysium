// Copyright © 2026 Jalapeno Labs

// Core
import { beforeEach, describe, expect, it, vi } from 'vitest'

// Misc
import {
  bindWorkspaceDock,
  listOpenWorkspacePanelIds,
  openWorkspacePanel,
  reportOpenWorkspacePanels,
  subscribeOpenWorkspacePanels,
} from './controller'

type Context = {
  openTab: ReturnType<typeof vi.fn<(kindId: string) => void>>
  activateTab: ReturnType<typeof vi.fn<(kindId: string) => void>>
  unbind: () => void
}

describe('workspace controller', () => {
  beforeEach<Context>((context) => {
    // The controller is module-level state; reset it to a known bound, empty
    // baseline before each test.
    reportOpenWorkspacePanels([])
    context.openTab = vi.fn<(kindId: string) => void>()
    context.activateTab = vi.fn<(kindId: string) => void>()
    context.unbind = bindWorkspaceDock({
      openTab: context.openTab,
      activateTab: context.activateTab,
    })
  })

  it<Context>('opens an unopened panel through the binding', (context) => {
    openWorkspacePanel('drive')
    expect(context.openTab).toHaveBeenCalledWith('drive')
    expect(context.activateTab).not.toHaveBeenCalled()
  })

  it<Context>('activates instead of reopening a panel the layout already has', (context) => {
    reportOpenWorkspacePanels([ 'drive' ])
    openWorkspacePanel('drive')
    expect(context.openTab).not.toHaveBeenCalled()
    expect(context.activateTab).toHaveBeenCalledWith('drive')
  })

  it<Context>('queues opens made while unbound and flushes them on bind', (context) => {
    context.unbind()

    openWorkspacePanel('drive')
    openWorkspacePanel('git')
    expect(context.openTab).not.toHaveBeenCalled()

    const lateOpenTab = vi.fn()
    const unbindLate = bindWorkspaceDock({
      openTab: lateOpenTab,
      activateTab: vi.fn(),
    })
    expect(lateOpenTab).toHaveBeenCalledTimes(2)
    expect(lateOpenTab).toHaveBeenNthCalledWith(1, 'drive')
    expect(lateOpenTab).toHaveBeenNthCalledWith(2, 'git')
    unbindLate()
  })

  it<Context>('ignores a stale unbind after a new workspace has bound', (context) => {
    const freshOpenTab = vi.fn()
    const unbindFresh = bindWorkspaceDock({
      openTab: freshOpenTab,
      activateTab: vi.fn(),
    })

    // The old workspace unmounts late; the fresh binding must survive.
    context.unbind()
    openWorkspacePanel('drive')
    expect(freshOpenTab).toHaveBeenCalledWith('drive')
    unbindFresh()
  })

  it<Context>('notifies subscribers only when the open set actually changes', () => {
    const listener = vi.fn()
    const unsubscribe = subscribeOpenWorkspacePanels(listener)

    reportOpenWorkspacePanels([ 'drive' ])
    expect(listener).toHaveBeenCalledTimes(1)
    expect(listOpenWorkspacePanelIds()).toEqual([ 'drive' ])

    reportOpenWorkspacePanels([ 'drive' ])
    expect(listener).toHaveBeenCalledTimes(1)

    reportOpenWorkspacePanels([])
    expect(listener).toHaveBeenCalledTimes(2)
    unsubscribe()
  })
})
