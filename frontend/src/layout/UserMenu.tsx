// Copyright © 2026 Jalapeno Labs

// Core
import { useTranslation } from 'react-i18next'

// Redux
import { selectMe } from '../store/authSlice'
import { useAppSelector } from '../store/hooks'

// User interface
import { Avatar, Button, Dropdown, Label, Separator, Tooltip } from '@heroui/react'
import { LuLogOut, LuShieldCheck, LuUser } from 'react-icons/lu'

// Misc
import { askToSignOut } from '../auth/askToSignOut'
import { useConfirm } from '../hooks/useConfirm'
import { UrlTree } from '../urls'
import { getInitials } from './userMenuPresentation'

// The signed-in person, at the right of the topbar (`~/.claude/docs/ux.md`, Navbars): who they
// are, then My account and Security as links, so either opens in a new tab too, then Sign out,
// last below a divider, which asks first.
export function UserMenu() {
  const { t } = useTranslation('navigation')
  const confirm = useConfirm()
  const me = useAppSelector(selectMe)

  // The auth gate renders the workspace only once it knows who is signed in.
  if (!me) {
    console.debug('UserMenu rendered before the signed-in person loaded')
    return null
  }

  return <Dropdown>
    <Tooltip delay={300}>
      <Button
        isIconOnly
        variant='ghost'
        aria-label={t('topbar.account', { name: me.user.name })}
      >
        <Avatar size='sm'>
          <Avatar.Fallback>{getInitials(me.user.name)}</Avatar.Fallback>
        </Avatar>
      </Button>
      <Tooltip.Content>
        <span>{t('topbar.accountTooltip')}</span>
      </Tooltip.Content>
    </Tooltip>
    <Dropdown.Popover placement='bottom end'>
      <div className='px-3 py-2'>
        <p className='text-sm font-bold'>{me.user.name}</p>
        <p className='text-xs italic opacity-70'>{me.user.email}</p>
      </div>
      <Separator />
      <Dropdown.Menu aria-label={t('topbar.accountTooltip')}>
        <Dropdown.Item id='account' textValue={t('topbar.myAccount')} href={UrlTree.settingsAccount}>
          <LuUser className='size-4' aria-hidden />
          <Label>{t('topbar.myAccount')}</Label>
        </Dropdown.Item>
        <Dropdown.Item id='security' textValue={t('topbar.security')} href={UrlTree.settingsSecurity}>
          <LuShieldCheck className='size-4' aria-hidden />
          <Label>{t('topbar.security')}</Label>
        </Dropdown.Item>
        <Separator />
        <Dropdown.Item id='signOut' textValue={t('topbar.signOut')} onAction={() => askToSignOut(confirm)}>
          <LuLogOut className='size-4' aria-hidden />
          <Label>{t('topbar.signOut')}</Label>
        </Dropdown.Item>
      </Dropdown.Menu>
    </Dropdown.Popover>
  </Dropdown>
}
