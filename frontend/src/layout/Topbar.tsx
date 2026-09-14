// Copyright © 2026 Jalapeno Labs

// Core
import { useEffect, useRef } from 'react'
import { useTranslation } from 'react-i18next'
import { useMatch } from 'react-router'

// User interface
import { buttonVariants, Kbd, Link, SearchField, Tooltip } from '@heroui/react'
import { LuSettings } from 'react-icons/lu'

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

  return <header className='level shrink-0 px-10 py-3'>
    <SearchField
      aria-label={t('common:search.label')}
      className='w-full max-w-sm'
    >
      <SearchField.Group className='bg-default shadow-none'>
        <SearchField.SearchIcon />
        <SearchField.Input
          ref={searchInputRef}
          placeholder={t('common:search.placeholder')}
        />
        <Kbd className='mr-2'>
          <Kbd.Content>{SEARCH_SHORTCUT_KEY}</Kbd.Content>
        </Kbd>
      </SearchField.Group>
    </SearchField>
    <div className='level-right'>
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
    </div>
  </header>
}
