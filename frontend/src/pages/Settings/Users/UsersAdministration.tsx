// Copyright © 2026 Jalapeno Labs

import type { User } from '../../../api/routes/userRoutes'

// Core
import { useState } from 'react'
import { useTranslation } from 'react-i18next'

// Redux
import { useAppSelector } from '../../../store/hooks'
import { selectPeopleByStanding } from '../../../store/usersSlice'
import { selectWorkspaceSettings } from '../../../store/workspaceSettingsSlice'

// User interface
import { Spinner, useOverlayState } from '@heroui/react'
import { PendingSignups } from './PendingSignups'
import { RecoveryLinkModal } from './RecoveryLinkModal'
import { UserTable } from './UserTable'
import { WorkspaceAccessSettings } from './WorkspaceAccessSettings'

// Misc
import { useUsersLoader, useWorkspaceSettingsLoader } from '../../../hooks/useServerData'
import { useUserActions } from './useUserActions'

// Everything an admin manages about people: who may join, who is waiting, and everyone's
// role and access.
export function UsersAdministration() {
  const { t } = useTranslation('users')
  const usersStatus = useUsersLoader()
  const settingsStatus = useWorkspaceSettingsLoader()
  const settings = useAppSelector(selectWorkspaceSettings)
  const people = useAppSelector(selectPeopleByStanding)
  const actions = useUserActions()

  const recoveryState = useOverlayState()
  const [ recoveryUser, setRecoveryUser ] = useState<User | null>(null)
  // Remounting the modal per opening makes a fresh link each time.
  const [ recoverySession, setRecoverySession ] = useState(0)

  function openRecoveryLink(user: User) {
    setRecoveryUser(user)
    setRecoverySession((session) => session + 1)
    recoveryState.open()
  }

  return <>
    <section className='relaxed'>
      <h2 className='text-xl font-semibold'>{t('access.heading')}</h2>
      <p className='compact mt-1 max-w-2xl text-sm opacity-70'>{t('access.description')}</p>
      {settings && <WorkspaceAccessSettings settings={settings} />}
      {!settings && settingsStatus === 'loading' && <Spinner size='sm' />}
      {settingsStatus === 'failed' && <p className='text-sm text-danger'>{t('access.loadError')}</p>}
    </section>

    {usersStatus === 'loading' && <div className='grid place-items-center py-16'>
      <Spinner />
    </div>}

    {usersStatus === 'failed' && <p className='py-10 text-center text-sm text-danger'>{
      t('table.loadError')
    }</p>}

    {usersStatus === 'loaded' && <>
      {people.pending.length > 0 && <PendingSignups
        users={people.pending}
        onApprove={actions.approve}
        onReject={actions.reject}
      />}
      <section>
        <h2 className='compact text-xl font-semibold'>{t('people.heading')}</h2>
        <UserTable
          users={people.approved}
          onChangeRole={actions.changeRole}
          onSetDisabled={actions.setDisabled}
          onRevokeSessions={actions.revokeSessions}
          onResetMfa={actions.resetMfa}
          onCreateRecoveryLink={openRecoveryLink}
        />
      </section>
    </>}

    {recoveryUser && <RecoveryLinkModal
      key={recoverySession}
      state={recoveryState}
      user={recoveryUser}
    />}
  </>
}
