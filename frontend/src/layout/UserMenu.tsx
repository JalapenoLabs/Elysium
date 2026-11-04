// Copyright © 2026 Jalapeno Labs

import type { Key } from '@heroui/react'

// Core
import { useTranslation } from 'react-i18next'
import { useNavigate } from 'react-router'

// Redux
import { selectMe } from '../store/authSlice'
import { useAppSelector } from '../store/hooks'

// User interface
import { Avatar, Button, Dropdown, Label, Separator } from '@heroui/react'
import { LuLogOut, LuShieldCheck, LuUser } from 'react-icons/lu'

// Misc
import { signOut } from '../auth/signOut'
import { UrlTree } from '../urls'
import { getInitials } from './userMenuPresentation'

// The signed-in person, at the right of the topbar: who they are, their profile and security
// settings, and signing out, last, below a divider.
export function UserMenu() {
  const { t } = useTranslation('navigation')
  const navigate = useNavigate()
  const me = useAppSelector(selectMe)

  if (!me) {
    return null
  }

  const actions: Record<string, () => void> = {
    profile: () => void navigate(UrlTree.settingsProfile),
    security: () => void navigate(UrlTree.settingsSecurity),
    signOut: () => void signOut(),
  }

  return <Dropdown>
    <Button
      isIconOnly
      variant='ghost'
      aria-label={t('topbar.account', { name: me.user.name })}
    >
      <Avatar size='sm'>
        <Avatar.Fallback>{getInitials(me.user.name)}</Avatar.Fallback>
      </Avatar>
    </Button>
    <Dropdown.Popover placement='bottom end'>
      <div className='border-b border-separator px-3 py-2'>
        <p className='text-sm font-semibold'>{me.user.name}</p>
        <p className='text-xs opacity-70'>{me.user.email}</p>
      </div>
      <Dropdown.Menu onAction={(key: Key) => actions[String(key)]?.()}>
        <Dropdown.Item id='profile' textValue={t('topbar.profile')}>
          <LuUser className='size-4' aria-hidden />
          <Label>{t('topbar.profile')}</Label>
        </Dropdown.Item>
        <Dropdown.Item id='security' textValue={t('topbar.security')}>
          <LuShieldCheck className='size-4' aria-hidden />
          <Label>{t('topbar.security')}</Label>
        </Dropdown.Item>
        <Separator />
        <Dropdown.Item id='signOut' textValue={t('topbar.signOut')}>
          <LuLogOut className='size-4' aria-hidden />
          <Label>{t('topbar.signOut')}</Label>
        </Dropdown.Item>
      </Dropdown.Menu>
    </Dropdown.Popover>
  </Dropdown>
}
