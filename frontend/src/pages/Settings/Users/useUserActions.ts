// Copyright © 2026 Jalapeno Labs

import type { PersonRole, User } from '../../../api/routes/userRoutes'

// Core
import { useTranslation } from 'react-i18next'

// Redux
import { useAppDispatch } from '../../../store/hooks'
import { userDeleted, userUpserted } from '../../../store/usersSlice'

// User interface
import { toast } from '@heroui/react'

// Utility
import { HTTPError } from 'ky'

// Misc
import { getApiErrorCode, getApiErrorMessage } from '../../../api/errors'
import {
  approveUser,
  rejectUser,
  resetUserMfa,
  revokeUserSessions,
  updateUser,
} from '../../../api/routes/userRoutes'
import { useConfirm } from '../../../hooks/useConfirm'

// The account conflicts the API names by code (409), and how each reads.
const conflictKeyByCode = {
  last_admin: 'conflicts.lastAdmin',
  account_state: 'conflicts.accountState',
} as const

// What an admin can do to a person's account, each confirmed where it cannot be taken back
// and reported with a toast. The API refuses anything that would leave the workspace without
// an active admin (409 `last_admin`), and that refusal is explained in its own words.
export function useUserActions() {
  const { t } = useTranslation([ 'users', 'common' ])
  const dispatch = useAppDispatch()
  const confirm = useConfirm()

  function reportFailure(error: unknown, context: string) {
    if (error instanceof HTTPError && error.response.status === 409) {
      const code = getApiErrorCode(error)
      if (code === 'last_admin' || code === 'account_state') {
        toast.danger(t(conflictKeyByCode[code]))
        return
      }
      toast.danger(getApiErrorMessage(error) ?? t('common:errors.unexpected'))
      return
    }
    console.debug(`useUserActions failed to ${context}`, { error })
    toast.danger(t('common:errors.unexpected'))
  }

  async function approve(user: User, role: PersonRole) {
    try {
      const response = await approveUser(user.id, role)
      dispatch(userUpserted(response.user))
      toast.success(t('toasts.approved', { name: user.name }))
    }
    catch (error) {
      reportFailure(error, 'approve a sign-up')
    }
  }

  function reject(user: User) {
    confirm({
      title: t('reject.title'),
      message: t('reject.message', { name: user.name, email: user.email }),
      tone: 'danger',
      confirmText: t('reject.confirm'),
      onConfirm: async () => {
        try {
          await rejectUser(user.id)
          dispatch(userDeleted(user.id))
          toast.success(t('toasts.rejected', { name: user.name }))
        }
        catch (error) {
          reportFailure(error, 'reject a sign-up')
          throw error
        }
      },
    })
  }

  async function changeRole(user: User, role: PersonRole) {
    try {
      const response = await updateUser(user.id, { role })
      dispatch(userUpserted(response.user))
      toast.success(t('toasts.roleChanged', { name: user.name }))
    }
    catch (error) {
      reportFailure(error, 'change a role')
    }
  }

  // Resolves whether the change was saved.
  async function saveDisabled(user: User, disabled: boolean) {
    try {
      const response = await updateUser(user.id, { disabled })
      dispatch(userUpserted(response.user))
      toast.success(disabled
        ? t('toasts.disabled', { name: user.name })
        : t('toasts.enabled', { name: user.name }))
      return true
    }
    catch (error) {
      reportFailure(error, 'disable or enable an account')
      return false
    }
  }

  // Disabling signs the person out and stops them signing in, so it asks first; enabling
  // does not.
  function setDisabled(user: User, disabled: boolean) {
    if (!disabled) {
      void saveDisabled(user, false)
      return
    }
    confirm({
      title: t('disable.title'),
      message: t('disable.message', { name: user.name }),
      tone: 'danger',
      confirmText: t('disable.confirm'),
      onConfirm: async () => {
        const isSaved = await saveDisabled(user, true)
        if (!isSaved) {
          // Keeps the dialog open so the admin can try again or cancel.
          throw new Error('The account was not disabled')
        }
      },
    })
  }

  function revokeSessions(user: User) {
    confirm({
      title: t('revokeSessions.title'),
      message: t('revokeSessions.message', { name: user.name }),
      confirmText: t('revokeSessions.confirm'),
      onConfirm: async () => {
        try {
          await revokeUserSessions(user.id)
          toast.success(t('toasts.sessionsRevoked', { name: user.name }))
        }
        catch (error) {
          reportFailure(error, 'sign someone out')
          throw error
        }
      },
    })
  }

  function resetMfa(user: User) {
    confirm({
      title: t('resetMfa.title'),
      message: t('resetMfa.message', { name: user.name }),
      tone: 'danger',
      confirmText: t('resetMfa.confirm'),
      onConfirm: async () => {
        try {
          await resetUserMfa(user.id)
          toast.success(t('toasts.mfaReset', { name: user.name }))
        }
        catch (error) {
          reportFailure(error, 'reset an authenticator')
          throw error
        }
      },
    })
  }

  return {
    approve,
    reject,
    changeRole,
    setDisabled,
    revokeSessions,
    resetMfa,
  } as const
}
