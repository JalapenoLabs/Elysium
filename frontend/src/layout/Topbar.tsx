// Copyright © 2026 Jalapeno Labs

// Core
import { useEffect, useRef } from 'react'
import { useTranslation } from 'react-i18next'
import { useMatch } from 'react-router'

// User interface
import { buttonVariants, Kbd, Link, SearchField, Tooltip } from '@heroui/react'
import { LuSettings } from 'react-icons/lu'
import { ElysiaToggle } from '../elysia/ElysiaToggle'
import { LiveUpdatesIndicator } from './LiveUpdatesIndicator'
import { MobileNavigation } from './MobileNavigation'
import { UserMenu } from './UserMenu'

// Misc
import { UrlTree } from '../urls'

// Pressing this key anywhere outside a text field focuses search, as in Stripe.
const SEARCH_SHORTCUT_KEY = '/'

export function Topbar() {
  const { t } = useTranslation([ 'navigation', 'common' ])
  const searchInputRef = useRef<HTMLInputElement>(null)
  const isInSettings = useMatch(`${UrlTree.settings}/*`)

  useEffect(() => {
    function onKeyDown(event: KeyboardEvent) {
      if (event.key !== SEARCH_SHORTCUT_KEY) {
        return
      }

      // Typing a slash into any field must still type a slash.
      const target = event.target
      if (target instanceof HTMLElement && target.closest('input, textarea, [contenteditable="true"]')) {
        return
      }

      event.preventDefault()
      searchInputRef.current?.focus()
    }

    window.addEventListener('keydown', onKeyDown)
    return () => window.removeEventListener('keydown', onKeyDown)
  }, [])

  return <header className='level shrink-0 gap-2 px-4 py-3 md:px-10'>
    <MobileNavigation />
    <SearchField
      aria-label={t('common:search.label')}
      className='w-full min-w-0 max-w-sm'
    >
      <SearchField.Group className='bg-default shadow-none'>
        <SearchField.SearchIcon />
        <SearchField.Input
          ref={searchInputRef}
          placeholder={t('common:search.placeholder')}
        />
        {/* Phones have no keyboard to press it on. */}
        <Kbd className='mr-2 hidden md:inline-flex'>
          <Kbd.Content>{SEARCH_SHORTCUT_KEY}</Kbd.Content>
        </Kbd>
      </SearchField.Group>
    </SearchField>
    <div className='level-right shrink-0 gap-1 md:gap-3'>
      <ElysiaToggle />
      <LiveUpdatesIndicator />
      <Tooltip delay={300}>
        {/* A link styled as a button: it navigates, so it must be an anchor. */}
        <Link
          href={UrlTree.settings}
          aria-label={t('topbar.settings')}
          className={buttonVariants({
            isIconOnly: true,
            variant: isInSettings
              ? 'secondary'
              : 'ghost',
            className: isInSettings
              ? 'text-accent'
              : undefined,
          })}
        >
          <LuSettings className='size-5' aria-hidden />
        </Link>
        <Tooltip.Content>
          <span>{
            t('topbar.settings')
          }</span>
        </Tooltip.Content>
      </Tooltip>
      <UserMenu />
    </div>
  </header>
}
