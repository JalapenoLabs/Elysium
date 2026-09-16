// Copyright © 2026 Jalapeno Labs

import type { GithubCredential } from '../../../api/routes/githubRoutes'

// Core
import { useTranslation } from 'react-i18next'
import { useNavigate } from 'react-router'

// Redux
import { useAppDispatch, useAppSelector } from '../../../store/hooks'
import {
  githubCredentialDeleted,
  githubCredentialUpserted,
  selectAllGithubCredentials,
} from '../../../store/githubCredentialsSlice'

// User interface
import { Breadcrumbs, Button, Spinner, toast } from '@heroui/react'
import { LuPlus } from 'react-icons/lu'
import { GithubCredentialTable } from './GithubCredentialTable'

// Misc
import { getApiErrorMessage } from '../../../api/errors'
import {
  deleteGithubCredential,
  testGithubCredential,
  updateGithubCredential,
} from '../../../api/routes/githubRoutes'
import { useConfirm } from '../../../hooks/useConfirm'
import { useGithubCredentialsLoader } from '../../../hooks/useServerData'
import { getGithubCredentialEditUrl, UrlTree } from '../../../urls'

// `/settings/github`: the GitHub tokens Elysium holds.
export function ManageGithubPage() {
  const { t } = useTranslation([ 'github', 'settings', 'common' ])
  const dispatch = useAppDispatch()
  const confirm = useConfirm()
  const credentials = useAppSelector(selectAllGithubCredentials)
  const status = useGithubCredentialsLoader()
  const navigate = useNavigate()

  async function runTokenTest(credential: GithubCredential) {
    try {
      const { result } = await testGithubCredential(credential.id)
      toast.success(t('toasts.testPassed', { name: credential.name, login: result.login }))
    }
    catch (error) {
      // A refused token answers 400 and an unreachable GitHub 502; both say what happened.
      const message = getApiErrorMessage(error)
      if (!message) {
        console.debug('ManageGithubPage failed to test a token', { error, credentialId: credential.id })
      }
      toast.danger(t('toasts.testFailed', { name: credential.name }), {
        description: message ?? t('common:errors.unexpected'),
      })
    }
  }

  // The token that was the default loses the flag through its own event on the stream.
  async function toggleDefault(credential: GithubCredential) {
    const isDefault = !credential.isDefault
    try {
      const response = await updateGithubCredential(credential.id, { isDefault })
      dispatch(githubCredentialUpserted(response.credential))
      const key = isDefault
        ? 'toasts.defaultSet'
        : 'toasts.defaultCleared'
      toast.success(t(key, { name: credential.name }))
    }
    catch (error) {
      console.debug('ManageGithubPage failed to change the default token', { error, credentialId: credential.id })
      toast.danger(t('common:errors.unexpected'), { description: getApiErrorMessage(error) ?? undefined })
    }
  }

  function confirmDelete(credential: GithubCredential) {
    confirm({
      title: t('delete.title', { name: credential.name }),
      message: t('delete.body'),
      tone: 'danger',
      confirmText: t('common:actions.delete'),
      onConfirm: async () => {
        try {
          await deleteGithubCredential(credential.id)
        }
        catch (error) {
          console.debug('ManageGithubPage failed to delete a token', { error, credentialId: credential.id })
          toast.danger(t('common:errors.unexpected'))
          // Keeps the dialog open to try again.
          throw error
        }

        dispatch(githubCredentialDeleted(credential.id))
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
          onPress={() => navigate(UrlTree.settingsGithubNew)}
        >
          <LuPlus className='size-4' aria-hidden />
          <span>{t('credentials.add')}</span>
        </Button>
      </div>

      <p className='relaxed text-sm opacity-70'>{t('page.defaultHint')}</p>

      {status === 'loading' && <div className='grid place-items-center py-16'>
        <Spinner />
      </div>}

      {status === 'failed' && <p className='py-10 text-center text-sm text-danger'>{
        t('table.loadError')
      }</p>}

      {status === 'loaded' && <GithubCredentialTable
        credentials={credentials}
        onEdit={(credential) => navigate(getGithubCredentialEditUrl(credential.id))}
        onTest={runTokenTest}
        onToggleDefault={toggleDefault}
        onDelete={confirmDelete}
      />}
    </section>
  </div>
}
