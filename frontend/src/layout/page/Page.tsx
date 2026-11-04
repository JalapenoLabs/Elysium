// Copyright © 2026 Jalapeno Labs

import type { ReactNode } from 'react'
import type { BlockerFunction } from 'react-router'
import type { PageHeaderProps } from './PageHeader'
import type { PageSave, PageSaveRegistry, PageSaveState } from './pageSave'

// Core
import { useCallback, useEffect, useEffectEvent, useMemo, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { useBlocker, useNavigate } from 'react-router'

// User interface
import { FloatingSaveCard } from './FloatingSaveCard'
import { LeavePageDialog } from './LeavePageDialog'
import { PageHeader } from './PageHeader'
import { PageSaveButton } from './PageSaveButton'

// Misc
import {
  canPressSave,
  leavesWithoutAsking,
  PageSaveContext,
  PageSaveStateContext,
  usePageSave,
} from './pageSave'
import { isModalOpen, readPageShortcut } from './pageShortcuts'
import { PageToolsContext } from './pageTools'
import { toast } from '../../toast/toast'

type Props = Omit<PageHeaderProps, 'save' | 'className' | 'toolsSlotRef'> & {
  // The page's save, for a page whose own component holds its form, such as a record edited
  // in place, whose fields sit in the header and beside other sections. A form deeper down
  // registers with `usePageSave` instead.
  save?: PageSave
  // Where Esc goes. A page with a Save defaults to its last parent, since every page that
  // edits something goes up a level on Esc; a page without one leaves Esc alone.
  escapeTo?: string
  children: ReactNode
}

// The frame of a standard page: the header (`PageHeader`), then the content. A form inside
// gives the page its one Save with `usePageSave`. The page runs it for every way of saving:
// the header's button, the card offered while that button is scrolled out of view, Ctrl+S,
// Enter in the form, and Save and continue when leaving. While the form is dirty, every way
// out of the page (a link, Esc, the browser's Back) asks first, and closing the tab warns.
// Saving is confirmed by the form with a toast alone, so nothing on the page moves. See
// docs/frontend.md, Page header.
export function Page(props: Props) {
  const { t } = useTranslation('common')
  const navigate = useNavigate()
  const [ save, setSave ] = useState<PageSave | null>(null)
  const [ isSaving, setIsSaving ] = useState(false)
  const [ isHeaderSaveVisible, setIsHeaderSaveVisible ] = useState(true)
  const [ isCardDismissed, setIsCardDismissed ] = useState(false)
  const [ isCreateFooterVisible, setIsCreateFooterVisible ] = useState(false)
  // The titlebar's slot for the page's list table, given to everything inside the page.
  const [ toolsContainer, setToolsContainer ] = useState<HTMLDivElement | null>(null)
  const headerSaveRef = useRef<HTMLDivElement>(null)
  // The blocker asks outside of rendering, so it reads the save and its progress from refs.
  const saveRef = useRef<PageSave | null>(null)
  const isSavingRef = useRef(false)
  // Set while Save and continue runs, so the form's own navigation after saving, such as to
  // the record it created, waits behind the blocker instead of taking the person elsewhere.
  const isLeavingRef = useRef(false)
  const hasSave = Boolean(save)
  const isDirty = Boolean(save?.isDirty)
  const { escapeTo, children, save: ownSave, ...headerProps } = props
  const backTo = escapeTo ?? props.parents?.at(-1)?.href
  const leaveTo = hasSave
    ? backTo
    : escapeTo

  const register = useCallback((next: PageSave | null) => {
    saveRef.current = next
    setSave(next)
  }, [])

  const runSave = useCallback(async () => {
    const current = saveRef.current
    if (!current || !canPressSave(current, isSavingRef.current)) {
      console.debug('Page ignored a save it cannot run', { hasSave: Boolean(current), isSaving: isSavingRef.current })
      return false
    }
    isSavingRef.current = true
    setIsSaving(true)
    try {
      return await current.save()
    }
    catch (error) {
      // Forms report their own refusals and resolve false; one that throws has a bug.
      console.debug('Page save threw instead of resolving', { error })
      toast.danger(t('errors.unexpected'))
      return false
    }
    finally {
      isSavingRef.current = false
      setIsSaving(false)
    }
  }, [ t ])

  const registry = useMemo<PageSaveRegistry>(() => ({ register, runSave }), [ register, runSave ])
  // A /new page's Create below its form stands in for the header's Save while it is in view,
  // so the floating card never offers a third button for the same save.
  const createFooterRef = useCallback((element: HTMLDivElement | null) => {
    if (!element) {
      return undefined
    }
    const observer = new IntersectionObserver(([ entry ]) => setIsCreateFooterVisible(entry.isIntersecting))
    observer.observe(element)
    return () => {
      observer.disconnect()
      setIsCreateFooterVisible(false)
    }
  }, [])

  const saveState = useMemo<PageSaveState>(
    () => ({ save, isSaving, runSave, backTo, createFooterRef }),
    [ save, isSaving, runSave, backTo, createFooterRef ],
  )

  const shouldBlock = useCallback<BlockerFunction>(({ currentLocation, nextLocation }) => {
    if (isLeavingRef.current) {
      return true
    }
    // A save in flight is the form's own: it may go on to the record or list once saved.
    if (isSavingRef.current || !saveRef.current?.isDirty || leavesWithoutAsking(nextLocation.state)) {
      return false
    }
    // A search change can swap what the page edits, such as Jira's `?project=`.
    return currentLocation.pathname !== nextLocation.pathname
      || currentLocation.search !== nextLocation.search
  }, [])
  const blocker = useBlocker(shouldBlock)

  async function saveAndContinue() {
    if (blocker.state !== 'blocked') {
      console.debug('Page was asked to save and continue with no navigation waiting')
      return
    }
    // Captured now: a navigation the form makes while saving replaces what the blocker
    // holds, and this one still goes where the person was heading.
    const { proceed, reset } = blocker
    isLeavingRef.current = true
    const isSaved = await runSave()
    isLeavingRef.current = false
    if (isSaved) {
      proceed()
      return
    }
    reset()
  }

  // Closing or reloading the tab is outside the router, so the browser asks instead.
  useEffect(() => {
    if (!isDirty) {
      return undefined
    }
    function warn(event: BeforeUnloadEvent) {
      event.preventDefault()
    }
    window.addEventListener('beforeunload', warn)
    return () => window.removeEventListener('beforeunload', warn)
  }, [ isDirty ])

  useEffect(() => {
    // A page without a Save has nothing to watch.
    const element = headerSaveRef.current
    if (!element) {
      return undefined
    }
    const observer = new IntersectionObserver(([ entry ]) => {
      setIsHeaderSaveVisible(entry.isIntersecting)
      // Dismissing lasts until Save is back in view, so the card returns on the next scroll.
      if (entry.isIntersecting) {
        setIsCardDismissed(false)
      }
    })
    observer.observe(element)
    return () => observer.disconnect()
  }, [ hasSave ])

  const onShortcut = useEffectEvent((event: KeyboardEvent) => {
    const shortcut = readPageShortcut(event)
    if (shortcut === 'save') {
      // Never the browser's own Save page dialog, even with nothing to save.
      event.preventDefault()
      if (save && canPressSave(save, isSaving) && !isModalOpen()) {
        void runSave()
      }
      return
    }
    // With unsaved changes, the blocker asks before this goes anywhere.
    if (shortcut === 'leave' && leaveTo) {
      void navigate(leaveTo)
    }
  })

  useEffect(() => {
    window.addEventListener('keydown', onShortcut)
    return () => window.removeEventListener('keydown', onShortcut)
  }, [])

  const showsFloatingCard = save !== null
    && !isHeaderSaveVisible
    && !isCreateFooterVisible
    && !isCardDismissed
    && save.isDirty

  return <PageSaveContext value={registry}>
    {ownSave && <OwnPageSave save={ownSave} />}
    <PageSaveStateContext value={saveState}>
      <div className='container'>
        <PageHeader
          {...headerProps}
          toolsSlotRef={setToolsContainer}
          save={save && <PageSaveButton
            ref={headerSaveRef}
            save={save}
            isSaving={isSaving}
            onPress={() => void runSave()}
          />}
        />
        <PageToolsContext value={toolsContainer}>
          {children}
        </PageToolsContext>
      </div>
    </PageSaveStateContext>

    {showsFloatingCard && <FloatingSaveCard
      save={save}
      isSaving={isSaving}
      onSave={() => void runSave()}
      onDismiss={() => setIsCardDismissed(true)}
    />}

    <LeavePageDialog
      isOpen={blocker.state === 'blocked'}
      isSaving={isSaving}
      saveBlockedReason={save?.blockedReason ?? null}
      onDiscard={() => blocker.proceed?.()}
      onSaveAndContinue={() => void saveAndContinue()}
      onKeepEditing={() => blocker.reset?.()}
    />
  </PageSaveContext>
}

type OwnPageSaveProps = {
  save: PageSave
}

// Registers a save passed to `Page` itself, from inside the page, as a form would.
function OwnPageSave(props: OwnPageSaveProps) {
  usePageSave(props.save)
  return null
}
