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
import { LuLogOut, LuUserPlus, LuUserRound, LuUsersRound } from 'react-icons/lu'

// Redux
import { useAppSelector } from '../../store/hooks'

// Misc
import { useSession } from '../../auth/useSession'
import { useCurrentUser } from '../../auth/useCurrentUser'
import { effectiveStatusOf } from './presence'
import { InvitePeopleModal } from './InvitePeopleModal'
import { switchAccount } from '../../api/routes/sessionRoutes'
import { resolveSession } from '../../auth/session'
import { UrlTree, getAddAccountUrl } from '../../urls'

// The signed-in identity in the taskbar: avatar + display name with the
// effective status dot, opening the account menu. The "Other accounts" section
// renders every other profile signed into this browser session straight from
// the session-context slice (seeded by `/context`, kept live by the realtime
// stream), with one-click switching.
export function ProfileBadge() {
  const { t } = useTranslation('common')
  const navigate = useNavigate()
  const session = useSession()
  const user = useCurrentUser()
  const accounts = useAppSelector((state) => state.sessionContext.accounts)

  const [ isInviteOpen, setIsInviteOpen ] = useState(false)

  const otherAccounts = accounts.filter((account) => !account.isActive)

  // One click flips the active account server-side; re-resolving reseeds the
  // whole session context for the new principal, and home renders in its
  // context.
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
    <Dropdown placement='top-start'>
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
      {/* Who you are comes first; everything else hangs off it. */}
      <DropdownSection showDivider>
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
      </DropdownSection>
      <DropdownSection title={t('account.otherAccounts')} showDivider>
        { [
          ...otherAccounts.map((account) =>
            <DropdownItem
              key={account.userId}
              textValue={account.email}
              onPress={() => onSwitchAccount(account.userId)}
            >
              <User
                name={account.displayName}
                description={account.email}
                avatarProps={{
                  size: 'sm',
                  name: account.displayName,
                  src: account.avatarUrl || undefined,
                }}
              />
            </DropdownItem>,
          ),
          <DropdownItem
            key='add-account'
            startContent={<LuUserPlus className='h-4 w-4' />}
            onPress={() => navigate(getAddAccountUrl())}
          >
            { t('account.addAccount') }
          </DropdownItem>,
        ] }
      </DropdownSection>
      <DropdownSection>
        <DropdownItem
          key='my-profile'
          startContent={<LuUserRound className='h-4 w-4' />}
          onPress={() => navigate(UrlTree.settingsProfile)}
        >
          { t('account.myProfile') }
        </DropdownItem>
        <DropdownItem
          key='invite-people'
          startContent={<LuUsersRound className='h-4 w-4' />}
          onPress={() => setIsInviteOpen(true)}
        >
          { t('invite.title') }
        </DropdownItem>
        <DropdownItem
          key='sign-out'
          color='danger'
          startContent={<LuLogOut className='h-4 w-4' />}
          onPress={session.signOut}
        >
          { t('actions.signOut') }
        </DropdownItem>
      </DropdownSection>
    </DropdownMenu>
    </Dropdown>
  </>
}
