// Copyright © 2026 Jalapeno Labs

// Core
import { useEffect } from 'react'
import { useTranslation } from 'react-i18next'

// Redux
import { useAppDispatch, useAppSelector } from '../store/hooks'
import {
  notificationsCleared,
  notificationsRead,
  selectNotifications,
  selectUnreadNotificationCount,
} from '../store/notificationsSlice'

// User interface
import { Button, Drawer } from '@heroui/react'
import { NotificationItem } from './NotificationItem'

type Props = {
  isOpen: boolean
  onOpenChange: (isOpen: boolean) => void
}

// Every toast the session kept, newest first, from the right edge. Whatever it shows has been
// seen, so the bell's count clears as it opens, and stays clear for anything that arrives while
// it is open.
export function NotificationDrawer(props: Props) {
  const { t } = useTranslation('navigation')
  const dispatch = useAppDispatch()
  const notifications = useAppSelector(selectNotifications)
  const unreadCount = useAppSelector(selectUnreadNotificationCount)

  useEffect(() => {
    if (props.isOpen && unreadCount) {
      dispatch(notificationsRead())
    }
  }, [ props.isOpen, unreadCount, dispatch ])

  // Controlled, so the root trigger wrapper is skipped, as in MobileNavigation.
  return <Drawer.Backdrop isOpen={props.isOpen} onOpenChange={props.onOpenChange}>
    <Drawer.Content placement='right'>
      <Drawer.Dialog className='gap-4'>
        <Drawer.Header>
          <Drawer.Heading className='text-lg'>{
            t('topbar.notifications.title')
          }</Drawer.Heading>
        </Drawer.Header>
        <Drawer.Body>
          {notifications.length
            ? <ul aria-label={t('topbar.notifications.title')} className='flex flex-col gap-2'>
              {notifications.map((notification) => <NotificationItem
                key={notification.id}
                notification={notification}
              />)}
            </ul>
            : <p className='py-10 text-center opacity-80'>{
              t('topbar.notifications.empty')
            }</p>}
        </Drawer.Body>
        {/* Each action on its own line, the drawer's one action first (ux.md, Drawers). */}
        <Drawer.Footer className='flex-col items-stretch'>
          <Button
            fullWidth
            isDisabled={!notifications.length}
            onPress={() => dispatch(notificationsCleared())}
          >
            <span>{
              t('topbar.notifications.clearAll')
            }</span>
          </Button>
          <Button
            fullWidth
            variant='secondary'
            onPress={() => props.onOpenChange(false)}
          >
            <span>{
              t('topbar.notifications.close')
            }</span>
          </Button>
        </Drawer.Footer>
      </Drawer.Dialog>
    </Drawer.Content>
  </Drawer.Backdrop>
}
