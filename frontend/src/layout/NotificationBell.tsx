// Copyright © 2026 Jalapeno Labs

// Core
import { useTranslation } from 'react-i18next'

// Redux
import { useAppSelector } from '../store/hooks'
import { selectUnreadNotificationCount } from '../store/notificationsSlice'

// User interface
import { Badge, Button, Tooltip, useOverlayState } from '@heroui/react'
import { LuBell } from 'react-icons/lu'
import { NotificationDrawer } from './NotificationDrawer'

// Misc
import { formatNumber } from '../format/numbers'

// The topbar's bell: how many notifications arrived since the drawer was last open, and the
// drawer itself.
export function NotificationBell() {
  const { t } = useTranslation('navigation')
  const drawer = useOverlayState()
  const unreadCount = useAppSelector(selectUnreadNotificationCount)

  // The badge is drawn for the eye alone, so the count is part of the button's name.
  const label = unreadCount
    ? t('topbar.notifications.labelUnread', { count: unreadCount })
    : t('topbar.notifications.title')

  return <>
    <Badge.Anchor>
      <Tooltip delay={300}>
        <Button
          isIconOnly
          variant='ghost'
          aria-label={label}
          onPress={drawer.open}
        >
          <LuBell className='size-5' aria-hidden />
        </Button>
        <Tooltip.Content>
          <span>{
            t('topbar.notifications.title')
          }</span>
        </Tooltip.Content>
      </Tooltip>
      {Boolean(unreadCount) && <Badge
        aria-hidden
        color='danger'
        size='sm'
        className='pointer-events-none'
      >{
          formatNumber(unreadCount)
        }</Badge>}
    </Badge.Anchor>
    <NotificationDrawer isOpen={drawer.isOpen} onOpenChange={drawer.setOpen} />
  </>
}
