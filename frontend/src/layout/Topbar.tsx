// Copyright © 2026 Jalapeno Labs

// Core
import { useTranslation } from 'react-i18next'
import { useMatch } from 'react-router'

// User interface
import { buttonVariants, Link, Tooltip } from '@heroui/react'
import { LuSettings } from 'react-icons/lu'
import { ElysiaToggle } from '../elysia/ElysiaToggle'
import { GlobalSearch } from '../search/GlobalSearch'
import { LiveUpdatesIndicator } from './LiveUpdatesIndicator'
import { MobileNavigation } from './MobileNavigation'
import { NotificationBell } from './NotificationBell'
import { UserMenu } from './UserMenu'

// Misc
import { UrlTree } from '../urls'

export function Topbar() {
  const { t } = useTranslation('navigation')
  const isInSettings = useMatch(`${UrlTree.settings}/*`)

  return <header className='level shrink-0 gap-2 px-4 py-3 md:px-10'>
    <MobileNavigation />
    <GlobalSearch />
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
      <NotificationBell />
      <UserMenu />
    </div>
  </header>
}
