// Copyright © 2026 Jalapeno Labs

import type { ReactNode } from 'react'
import type { PageHeaderProps } from './PageHeader'
import type { PageSave } from './pageSave'

// Core
import { useEffect, useEffectEvent, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { useNavigate } from 'react-router'

// User interface
import { FloatingSaveCard } from './FloatingSaveCard'
import { PageHeader } from './PageHeader'
import { PageSaveButton } from './PageSaveButton'

// Misc
import { useConfirm } from '../../hooks/useConfirm'
import { canPressSave, PageSaveContext, submitPageForm } from './pageSave'
import { isModalOpen, readPageShortcut } from './pageShortcuts'

type Props = Omit<PageHeaderProps, 'save' | 'className'> & {
  // Where Esc goes, asking first while the form is dirty. Omit to leave Esc alone.
  escapeTo?: string
  children: ReactNode
}

// The frame of a standard page: the header (`PageHeader`), then the content. A form inside
// gives the page its one Save with `usePageSave`, which the header shows, Ctrl+S presses,
// and a card in the bottom right offers while the header's Save is scrolled out of view,
// until dismissed. Saving is confirmed by the form with a toast alone, so nothing on the page
// moves. See docs/frontend.md, Page header.
export function Page(props: Props) {
  const { t } = useTranslation('common')
  const navigate = useNavigate()
  const confirm = useConfirm()
  const [ save, setSave ] = useState<PageSave | null>(null)
  const [ isHeaderSaveVisible, setIsHeaderSaveVisible ] = useState(true)
  const [ isCardDismissed, setIsCardDismissed ] = useState(false)
  const headerSaveRef = useRef<HTMLDivElement>(null)
  const hasSave = Boolean(save)
  const { escapeTo, children, ...headerProps } = props

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
      if (save && canPressSave(save) && !isModalOpen()) {
        submitPageForm(save)
      }
      return
    }
    if (shortcut !== 'leave' || !escapeTo) {
      return
    }
    if (!save?.isDirty) {
      void navigate(escapeTo)
      return
    }
    confirm({
      title: t('page.leave.title'),
      message: t('page.leave.message'),
      tone: 'danger',
      confirmText: t('page.leave.confirm'),
      onConfirm: () => void navigate(escapeTo),
    })
  })

  useEffect(() => {
    window.addEventListener('keydown', onShortcut)
    return () => window.removeEventListener('keydown', onShortcut)
  }, [])

  const showsFloatingCard = save !== null && !isHeaderSaveVisible && !isCardDismissed && save.isDirty

  return <PageSaveContext value={setSave}>
    <div className='container'>
      <PageHeader
        {...headerProps}
        save={save && <PageSaveButton ref={headerSaveRef} save={save} />}
      />
      {children}
    </div>

    {showsFloatingCard && <FloatingSaveCard
      save={save}
      onDismiss={() => setIsCardDismissed(true)}
    />}
  </PageSaveContext>
}
