// Copyright © 2026 Jalapeno Labs

// Core
import { useState } from 'react'
import { useNavigate } from 'react-router'
import { useTranslation } from 'react-i18next'

// UI
import {
  Avatar,
  Badge,
  Button,
  Dropdown,
  DropdownItem,
  DropdownMenu,
  DropdownSection,
  DropdownTrigger,
  User,
} from '@heroui/react'

// Misc
import { useSession } from '../../auth/useSession'
import { useCurrentUser } from '../../auth/useCurrentUser'
import { effectiveStatusOf } from './presence'
import { InvitePeopleModal } from './InvitePeopleModal'
import { listSessionAccounts, switchAccount, type SessionAccount } from '../../api/routes/sessionRoutes'
import { resolveSession } from '../../auth/session'
import { UrlTree, getAddAccountUrl } from '../../urls'

// The signed-in identity in the navbar: avatar + display name with the
// effective status dot, opening the account menu. The "Other accounts" section
// lists every other profile signed into this browser session with one-click
// switching; it degrades to just "Add account" while the multi-session
// endpoints aren't reachable.
export function ProfileBadge() {
  const { t } = useTranslation('common')
  const navigate = useNavigate()
  const session = useSession()
  const user = useCurrentUser()

  const [ isInviteOpen, setIsInviteOpen ] = useState(false)
  const [ otherAccounts, setOtherAccounts ] = useState<SessionAccount[]>([])

  // Refreshed on every open so the list reflects accounts added or signed out
  // in other tabs; a failure (including the endpoints not being live yet)
  // simply leaves the section with "Add account".
  async function onMenuOpenChange(isOpen: boolean) {
    if (!isOpen) {
      return
    }
    try {
      const sessionAccounts = await listSessionAccounts()
      setOtherAccounts(sessionAccounts.accounts.filter((account) => !account.is_active))
    }
    catch (error) {
      console.debug('session accounts unavailable', { error })
      setOtherAccounts([])
    }
  }

  // One click flips the active account server-side; re-resolving the session
  // swaps the principal (which also drops the cached profile) and home renders
  // in the newly active account's context.
  async function onSwitchAccount(userId: string) {
    try {
      await switchAccount(userId)
      await resolveSession()
      navigate(UrlTree.home)
    }
    catch (error) {
      console.debug('account switch failed', { error })
    }
  }

  // The session principal keeps the badge functional (especially sign-out)
  // while the full profile is still loading or unavailable.
  const displayName = user?.displayName || session.principal?.username || ''
  const email = user?.email || session.principal?.email || ''
  const avatarUrl = user?.avatarUrl || undefined
  const status = user ? effectiveStatusOf(user) : null

  // The identity block shows the person's full name where one exists; the
  // display name everywhere else in the product stays the render field.
  const fullName = [ user?.givenName, user?.familyName ]
    .filter(Boolean)
    .join(' ') || displayName

  return <>
    <InvitePeopleModal
      isOpen={isInviteOpen}
      onOpenChange={setIsInviteOpen}
    />
    <Dropdown placement='bottom-end' onOpenChange={onMenuOpenChange}>
    <DropdownTrigger>
      <Button variant='light' className='h-12'>
        { status
          ? <Badge
            content=''
            shape='circle'
            placement='bottom-right'
            className={`${status.dotColor} border-transparent`}
          >
            <Avatar size='sm' name={displayName} src={avatarUrl} />
          </Badge>
          : <Avatar size='sm' name={displayName} src={avatarUrl} />
        }
        <span>{ displayName }</span>
      </Button>
    </DropdownTrigger>
    <DropdownMenu aria-label={t('account.menuLabel')}>
      <DropdownSection title={t('account.otherAccounts')} showDivider>
        { [
          ...otherAccounts.map((account) =>
            <DropdownItem
              key={account.user_id}
              textValue={account.email}
              onPress={() => onSwitchAccount(account.user_id)}
            >
              <User
                name={account.display_name}
                description={account.email}
                avatarProps={{
                  size: 'sm',
                  name: account.display_name,
                  src: account.avatar_url || undefined,
                }}
              />
            </DropdownItem>,
          ),
          <DropdownItem
            key='add-account'
            onPress={() => navigate(getAddAccountUrl())}
          >
            { t('account.addAccount') }
          </DropdownItem>,
        ] }
      </DropdownSection>
      <DropdownSection>
        <DropdownItem key='identity' isReadOnly textValue={fullName}>
          <User
            name={fullName}
            description={email}
            avatarProps={{
              size: 'sm',
              name: displayName,
              src: avatarUrl,
            }}
          />
        </DropdownItem>
        <DropdownItem
          key='my-profile'
          onPress={() => navigate(UrlTree.settingsProfile)}
        >
          { t('account.myProfile') }
        </DropdownItem>
        <DropdownItem
          key='invite-people'
          onPress={() => setIsInviteOpen(true)}
        >
          { t('invite.title') }
        </DropdownItem>
        <DropdownItem
          key='sign-out'
          color='danger'
          onPress={session.signOut}
        >
          { t('actions.signOut') }
        </DropdownItem>
      </DropdownSection>
    </DropdownMenu>
    </Dropdown>
  </>
}
