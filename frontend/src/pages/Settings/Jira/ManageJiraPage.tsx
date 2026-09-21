// Copyright © 2026 Jalapeno Labs

import type { JiraCredential } from '../../../api/routes/jiraRoutes'

// Core
import { useTranslation } from 'react-i18next'
import { useNavigate } from 'react-router'

// Redux
import { useAppDispatch, useAppSelector } from '../../../store/hooks'
import {
  jiraCredentialDeleted,
  jiraCredentialUpserted,
  selectAllJiraCredentials,
} from '../../../store/jiraCredentialsSlice'

// User interface
import { Breadcrumbs, Button, Spinner, toast } from '@heroui/react'
import { LuPlus } from 'react-icons/lu'
import { JiraCredentialTable } from './JiraCredentialTable'

// Misc
import { getApiErrorMessage } from '../../../api/errors'
import { deleteJiraCredential, testJiraCredential } from '../../../api/routes/jiraRoutes'
import { useConfirm } from '../../../hooks/useConfirm'
import { useJiraCredentialsLoader } from '../../../hooks/useServerData'
import { getJiraCredentialEditUrl, getJiraDoneTransitionsUrl, UrlTree } from '../../../urls'

// `/settings/jira`: the Jira Cloud sites Elysium reads.
export function ManageJiraPage() {
  const { t } = useTranslation([ 'jira', 'settings', 'common' ])
  const dispatch = useAppDispatch()
  const confirm = useConfirm()
  const credentials = useAppSelector(selectAllJiraCredentials)
  const status = useJiraCredentialsLoader()
  const navigate = useNavigate()

  async function runConnectionTest(credential: JiraCredential) {
    try {
      const { credential: checked, result } = await testJiraCredential(credential.id)
      // The test records a fresh account and check time, which the row shows at once
      // rather than waiting for the same credential to arrive on the event stream.
      dispatch(jiraCredentialUpserted(checked))

      // An allowed project or board Jira no longer hands over is the whole reason to
      // test, so it is said out loud rather than left to the edit page.
      const unreachable: string[] = []
      for (const project of result.projects) {
        if (!project.reachable) {
          unreachable.push(project.key)
        }
      }
      for (const board of result.boards) {
        if (!board.reachable) {
          unreachable.push(board.name)
        }
      }

      if (unreachable.length) {
        toast.warning(t('toasts.testUnreachable', { name: credential.name, count: unreachable.length }), {
          description: unreachable.join(', '),
        })
        return
      }

      toast.success(t('toasts.testPassed', {
        name: credential.name,
        account: result.account.displayName,
      }))
    }
    catch (error) {
      // A refused token answers 400 and an unreachable Jira 502; both say what happened.
      const message = getApiErrorMessage(error)
      if (!message) {
        console.debug('ManageJiraPage failed to test a credential', { error, credentialId: credential.id })
      }
      toast.danger(t('toasts.testFailed', { name: credential.name }), {
        description: message ?? t('common:errors.unexpected'),
      })
    }
  }

  function confirmDelete(credential: JiraCredential) {
    confirm({
      title: t('delete.title', { name: credential.name }),
      message: t('delete.body'),
      tone: 'danger',
      confirmText: t('common:actions.delete'),
      onConfirm: async () => {
        try {
          await deleteJiraCredential(credential.id)
        }
        catch (error) {
          console.debug('ManageJiraPage failed to delete a credential', { error, credentialId: credential.id })
          toast.danger(t('common:errors.unexpected'))
          // Keeps the dialog open to try again.
          throw error
        }

        dispatch(jiraCredentialDeleted(credential.id))
        toast.success(t('toasts.deleted', { name: credential.name }))
      },
    })
  }

  return <div className='container'>
    <Breadcrumbs className='compact'>
      <Breadcrumbs.Item href={UrlTree.settings}>{t('settings:title')}</Breadcrumbs.Item>
      <Breadcrumbs.Item>{t('title')}</Breadcrumbs.Item>
    </Breadcrumbs>
    <section>
      <div className='level relaxed items-center'>
        <h1 className='text-3xl font-bold'>{
          t('title')
        }</h1>
        <Button
          size='sm'
          variant='outline'
          className='shrink-0'
          onPress={() => navigate(UrlTree.settingsJiraNew)}
        >
          <LuPlus className='size-4' aria-hidden />
          <span>{t('credentials.add')}</span>
        </Button>
      </div>

      <p className='relaxed text-sm opacity-70'>{t('page.hint')}</p>

      {status === 'loading' && <div className='grid place-items-center py-16'>
        <Spinner />
      </div>}

      {status === 'failed' && <p className='py-10 text-center text-sm text-danger'>{
        t('table.loadError')
      }</p>}

      {status === 'loaded' && <JiraCredentialTable
        credentials={credentials}
        onEdit={(credential) => navigate(getJiraCredentialEditUrl(credential.id))}
        onTest={runConnectionTest}
        onDoneTransitions={(credential) => navigate(getJiraDoneTransitionsUrl(credential.id))}
        onDelete={confirmDelete}
      />}
    </section>
  </div>
}
