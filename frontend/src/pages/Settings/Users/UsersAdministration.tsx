// Copyright © 2026 Jalapeno Labs

import type { PersonRole, User } from '../../../api/routes/userRoutes'
import type { AccessSettings } from './WorkspaceAccessSettings'

// Core
import { useState } from 'react'
import { useTranslation } from 'react-i18next'

// Redux
import { useAppDispatch, useAppSelector } from '../../../store/hooks'
import { selectPeopleByStanding } from '../../../store/usersSlice'
import { selectWorkspaceSettings, workspaceSettingsUpdated } from '../../../store/workspaceSettingsSlice'

// User interface
import { Spinner, useOverlayState } from '@heroui/react'
import { PendingSignups } from './PendingSignups'
import { RecoveryLinkModal } from './RecoveryLinkModal'
import { UserTable } from './UserTable'
import { WorkspaceAccessSettings } from './WorkspaceAccessSettings'

// Misc
import { updateWorkspaceSettings } from '../../../api/routes/userRoutes'
import { useUsersLoader, useWorkspaceSettingsLoader } from '../../../hooks/useServerData'
import { usePageSave } from '../../../layout/page/pageSave'
import { useUserActions } from './useUserActions'
import { toast } from '../../../toast/toast'

// Everything an admin manages about people: who may join, who is waiting, and everyone's
// role and access. The access switches and roles are the page's fields, kept by its Save;
// only what is changed here is held and sent, so a change another admin makes meanwhile
// shows at once and is never undone. Approving, disabling, and the rest act at once.
export function UsersAdministration() {
  const { t } = useTranslation([ 'users', 'common' ])
  const dispatch = useAppDispatch()
  const usersStatus = useUsersLoader()
  const settingsStatus = useWorkspaceSettingsLoader()
  const settings = useAppSelector(selectWorkspaceSettings)
  const people = useAppSelector(selectPeopleByStanding)
  const actions = useUserActions()
  const [ accessChanges, setAccessChanges ] = useState<Partial<AccessSettings>>({})
  const [ roleChanges, setRoleChanges ] = useState<Record<string, PersonRole>>({})

  const isAccessChanged = Boolean(settings) && (
    (accessChanges.signupOpen ?? settings?.signupOpen) !== settings?.signupOpen
    || (accessChanges.requireMfa ?? settings?.requireMfa) !== settings?.requireMfa
  )
  const changedRoles = people.approved.filter(
    (user) => roleChanges[user.id] && roleChanges[user.id] !== user.role,
  )

  async function save() {
    let isSaved = true
    if (isAccessChanged) {
      try {
        const response = await updateWorkspaceSettings(accessChanges)
        dispatch(workspaceSettingsUpdated(response.settings))
        setAccessChanges({})
        toast.success(t('access.saved'))
      }
      catch (error) {
        console.debug('UsersAdministration could not save the access settings', { error })
        toast.danger(t('common:errors.unexpected'))
        isSaved = false
      }
    }
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
    isDirty: isAccessChanged || changedRoles.length > 0,
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

  return <>
    <section className='relaxed'>
      <h2 className='text-xl font-semibold'>{t('access.heading')}</h2>
      <p className='compact mt-1 max-w-2xl text-sm opacity-70'>{t('access.description')}</p>
      {settings && <WorkspaceAccessSettings
        settings={settings}
        changes={accessChanges}
        onChange={setAccessChanges}
      />}
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
          roleChanges={roleChanges}
          onChangeRole={(user, role) => setRoleChanges({ ...roleChanges, [user.id]: role })}
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
