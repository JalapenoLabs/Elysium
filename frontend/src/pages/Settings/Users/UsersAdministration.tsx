// Copyright © 2026 Jalapeno Labs

import type { PersonRole, User } from '../../../api/routes/userRoutes'

// Core
import { useState } from 'react'
import { useTranslation } from 'react-i18next'

// Redux
import { useAppSelector } from '../../../store/hooks'
import { selectPeopleByStanding } from '../../../store/usersSlice'

// User interface
import { Spinner, Tabs, useOverlayState } from '@heroui/react'
import { EmptyNotice } from '../../../components/EmptyNotice'
import { PageTabs } from '../../../components/PageTabs'
import { PendingSignups } from './PendingSignups'
import { RecoveryLinkModal } from './RecoveryLinkModal'
import { UserTable } from './UserTable'

// Misc
import { useUsersLoader } from '../../../hooks/useServerData'
import { useTabParam } from '../../../hooks/useTabParam'
import { usePageSave } from '../../../layout/page/pageSave'
import { useUserActions } from './useUserActions'

const TABS = [ 'people', 'signups' ] as const

// Everyone approved, and on a tab of their own the sign-ups waiting for an admin. Roles are
// the page's fields, kept by its Save; only what is changed here is held and sent, so a
// change another admin makes meanwhile shows at once and is never undone. Approving,
// disabling, and the rest act at once. The open tab lives in the address.
export function UsersAdministration() {
  const { t } = useTranslation([ 'users', 'common' ])
  const usersStatus = useUsersLoader()
  const people = useAppSelector(selectPeopleByStanding)
  const actions = useUserActions()
  const [ selectedTab, selectTab ] = useTabParam(TABS)
  const [ roleChanges, setRoleChanges ] = useState<Record<string, PersonRole>>({})

  const changedRoles = people.approved.filter(
    (user) => roleChanges[user.id] && roleChanges[user.id] !== user.role,
  )

  async function save() {
    let isSaved = true
    // A role the API refuses, such as the last admin's, stays picked so it can be fixed.
    const refusedRoles: Record<string, PersonRole> = {}
    for (const user of changedRoles) {
      const role = roleChanges[user.id]
      if (!await actions.changeRole(user, role)) {
        refusedRoles[user.id] = role
        isSaved = false
      }
    }
    setRoleChanges(refusedRoles)
    return isSaved
  }

  usePageSave({
    save,
    isDirty: changedRoles.length > 0,
  })

  const recoveryState = useOverlayState()
  const [ recoveryUser, setRecoveryUser ] = useState<User | null>(null)
  // Remounting the modal per opening makes a fresh link each time.
  const [ recoverySession, setRecoverySession ] = useState(0)

  function openRecoveryLink(user: User) {
    setRecoveryUser(user)
    setRecoverySession((session) => session + 1)
    recoveryState.open()
  }

  if (usersStatus === 'loading') {
    return <div className='grid place-items-center py-16'>
      <Spinner />
    </div>
  }
  if (usersStatus === 'failed') {
    return <p className='py-10 text-center text-sm text-danger'>{t('table.loadError')}</p>
  }

  return <>
    <PageTabs
      label={t('tabs.label')}
      selectedKey={selectedTab}
      onSelectionChange={selectTab}
      tabs={[
        { id: 'people', label: t('tabs.people'), count: people.approved.length },
        { id: 'signups', label: t('tabs.signups'), count: people.pending.length, isWaiting: true },
      ]}
    >
      <Tabs.Panel id='people'>
        <UserTable
          users={people.approved}
          roleChanges={roleChanges}
          onChangeRole={(user, role) => setRoleChanges({ ...roleChanges, [user.id]: role })}
          onSetDisabled={actions.setDisabled}
          onRevokeSessions={actions.revokeSessions}
          onResetMfa={actions.resetMfa}
          onCreateRecoveryLink={openRecoveryLink}
        />
      </Tabs.Panel>
      <Tabs.Panel id='signups'>{
        people.pending.length
          ? <PendingSignups
            users={people.pending}
            onApprove={actions.approve}
            onReject={actions.reject}
          />
          : <EmptyNotice>{t('pending.empty')}</EmptyNotice>
      }</Tabs.Panel>
    </PageTabs>

    {recoveryUser && <RecoveryLinkModal
      key={recoverySession}
      state={recoveryState}
      user={recoveryUser}
    />}
  </>
}
