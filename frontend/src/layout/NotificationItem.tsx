// Copyright © 2026 Jalapeno Labs

import type { ParseKeys } from 'i18next'
import type { IconType } from 'react-icons'
import type { Notification } from '../store/notificationsSlice'
import type { ToastVariant } from '../toast/toastPolicy'

// Core
import { useTranslation } from 'react-i18next'

// Redux
import { useAppDispatch } from '../store/hooks'
import { notificationCleared } from '../store/notificationsSlice'

// User interface
import { Button, Link, Tooltip } from '@heroui/react'
import { LuCircleAlert, LuCircleCheck, LuExternalLink, LuInfo, LuTriangleAlert, LuX } from 'react-icons/lu'

// Misc
import { formatShortDateTime } from '../format/dates'

// How each kind of toast is told apart in the list: an edge and an icon in its status color,
// and a name for the icon, so the status is not carried by color alone.
const presentationByVariant = {
  default: {
    icon: LuInfo,
    color: 'text-accent',
    edge: 'border-accent',
    labelKey: 'topbar.notifications.variants.default',
  },
  success: {
    icon: LuCircleCheck,
    color: 'text-success',
    edge: 'border-success',
    labelKey: 'topbar.notifications.variants.success',
  },
  warning: {
    icon: LuTriangleAlert,
    color: 'text-warning',
    edge: 'border-warning',
    labelKey: 'topbar.notifications.variants.warning',
  },
  danger: {
    icon: LuCircleAlert,
    color: 'text-danger',
    edge: 'border-danger',
    labelKey: 'topbar.notifications.variants.danger',
  },
} as const satisfies Record<ToastVariant, {
  icon: IconType
  color: string
  edge: string
  labelKey: ParseKeys<'navigation'>
}>

type Props = {
  notification: Notification
}

// One kept toast: what it said, its link, when it was shown, and a button to clear it.
export function NotificationItem(props: Props) {
  const { t } = useTranslation('navigation')
  const dispatch = useAppDispatch()
  const { notification } = props
  const presentation = presentationByVariant[notification.variant]
  const Icon = presentation.icon

  return <li className={`flex items-start gap-3 rounded-xl border-l-4 bg-surface p-3 ${presentation.edge}`}>
    <Icon
      role='img'
      aria-label={t(presentation.labelKey)}
      className={`mt-0.5 size-4 shrink-0 ${presentation.color}`}
    />
    {/* The drawer's body mutes its text, which next to a link reads as one; the theme's own
        color, dimmed by opacity, keeps the description apart from the link. */}
    <div className='min-w-0 flex-1 break-words text-foreground'>
      <p className='font-medium'>{notification.title}</p>
      {notification.description && <p className='opacity-80'>{notification.description}</p>}
      {notification.link && <Link
        href={notification.link.href}
        target='_blank'
        rel='noreferrer'
        className='text-link'
      >
        <span>{notification.link.label}</span>
        <LuExternalLink className='size-3' aria-hidden />
      </Link>}
      <time dateTime={notification.createdAt} className='mt-1 block text-xs opacity-60'>{
        formatShortDateTime(notification.createdAt)
      }</time>
    </div>
    <Tooltip delay={300}>
      <Button
        isIconOnly
        size='sm'
        variant='ghost'
        aria-label={t('topbar.notifications.clearOne', { title: notification.title })}
        onPress={() => dispatch(notificationCleared(notification.id))}
      >
        <LuX className='size-4' aria-hidden />
      </Button>
      <Tooltip.Content>
        <span>{
          t('topbar.notifications.clear')
        }</span>
      </Tooltip.Content>
    </Tooltip>
  </li>
}
