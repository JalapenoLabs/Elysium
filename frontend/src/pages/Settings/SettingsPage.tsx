// Copyright © 2026 Jalapeno Labs

import type { ReactNode } from 'react'
import type { SettingsSave } from './settingsSave'

// Core
import { useEffect, useEffectEvent, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { useNavigate } from 'react-router'

// User interface
import { Breadcrumbs } from '@heroui/react'
import { FloatingSaveCard } from './FloatingSaveCard'
import { SettingsSaveButton } from './SettingsSaveButton'

// Misc
import { useConfirm } from '../../hooks/useConfirm'
import { UrlTree } from '../../urls'
import { canPressSave, SettingsSaveContext, submitSettingsForm } from './settingsSave'
import { isModalOpen, readSettingsShortcut } from './settingsShortcuts'

export type SettingsCrumb = {
  label: string
  href: string
}

type Props = {
  title: string
  // The pages between Settings and this one, such as Manage LLMs above an LLM's edit page.
  // Esc goes back to the last of them, or to Settings.
  parents?: SettingsCrumb[]
  // Shown left of Save in the header, such as a list page's Add button.
  actions?: ReactNode
  children: ReactNode
}

// The frame of every settings subpage: breadcrumbs, the title on the left, and on the right
// the page's actions and its one Save, which a form inside registers with `useSettingsSave`.
// Esc goes back to the parent page and Ctrl+S presses Save. While the header's Save is
// scrolled out of view, a card in the bottom right offers it, until dismissed. Saving is
// confirmed by the form with a toast alone, so nothing on the page moves. See
// docs/frontend.md, Settings pages.
export function SettingsPage(props: Props) {
  const { t } = useTranslation('settings')
  const navigate = useNavigate()
  const confirm = useConfirm()
  const [ save, setSave ] = useState<SettingsSave | null>(null)
  const [ isHeaderSaveVisible, setIsHeaderSaveVisible ] = useState(true)
  const [ isCardDismissed, setIsCardDismissed ] = useState(false)
  const headerSaveRef = useRef<HTMLDivElement>(null)
  const parentHref = props.parents?.at(-1)?.href ?? UrlTree.settings
  const hasSave = Boolean(save)

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
    const shortcut = readSettingsShortcut(event)
    if (shortcut === 'save') {
      // Never the browser's own Save page dialog, even with nothing to save.
      event.preventDefault()
      if (save && canPressSave(save) && !isModalOpen()) {
        submitSettingsForm(save)
      }
      return
    }
    if (shortcut !== 'leave') {
      return
    }
    if (!save?.isDirty) {
      void navigate(parentHref)
      return
    }
    confirm({
      title: t('page.leave.title'),
      message: t('page.leave.message'),
      tone: 'danger',
      confirmText: t('page.leave.confirm'),
      onConfirm: () => void navigate(parentHref),
    })
  })

  useEffect(() => {
    window.addEventListener('keydown', onShortcut)
    return () => window.removeEventListener('keydown', onShortcut)
  }, [])

  const showsFloatingCard = save !== null && !isHeaderSaveVisible && !isCardDismissed && save.isDirty

  return <SettingsSaveContext value={setSave}>
    <div className='container'>
      <Breadcrumbs className='compact'>
        <Breadcrumbs.Item href={UrlTree.settings}>{t('title')}</Breadcrumbs.Item>
        {props.parents?.map((parent) => <Breadcrumbs.Item key={parent.href} href={parent.href}>{
          parent.label
        }</Breadcrumbs.Item>)}
        <Breadcrumbs.Item>{props.title}</Breadcrumbs.Item>
      </Breadcrumbs>
      <header className='relaxed level flex-wrap'>
        <h1 className='min-w-0 text-3xl font-bold'>{
          props.title
        }</h1>
        {(props.actions || save) && <div className='level-right flex-wrap'>
          {props.actions}
          {save && <SettingsSaveButton ref={headerSaveRef} save={save} />}
        </div>}
      </header>

      {props.children}
    </div>

    {showsFloatingCard && <FloatingSaveCard
      save={save}
      onDismiss={() => setIsCardDismissed(true)}
    />}
  </SettingsSaveContext>
}
