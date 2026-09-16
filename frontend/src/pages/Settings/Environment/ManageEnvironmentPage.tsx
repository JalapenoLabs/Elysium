// Copyright © 2026 Jalapeno Labs

import type { EnvironmentVariable } from '../../../api/routes/environmentRoutes'

// Core
import { useTranslation } from 'react-i18next'
import { useNavigate } from 'react-router'

// Redux
import { useAppDispatch, useAppSelector } from '../../../store/hooks'
import { environmentVariableDeleted, selectAllEnvironmentVariables } from '../../../store/environmentVariablesSlice'

// User interface
import { Breadcrumbs, Button, Spinner, toast } from '@heroui/react'
import { LuPlus } from 'react-icons/lu'
import { EnvironmentVariableTable } from './EnvironmentVariableTable'
import { EnvironmentVisibilityNote } from './EnvironmentVisibilityNote'

// Misc
import { deleteEnvironmentVariable } from '../../../api/routes/environmentRoutes'
import { useConfirm } from '../../../hooks/useConfirm'
import { useEnvironmentVariablesLoader } from '../../../hooks/useServerData'
import { getEnvironmentVariableEditUrl, UrlTree } from '../../../urls'

// `/settings/environment`: the global environment variables every coding session receives.
export function ManageEnvironmentPage() {
  const { t } = useTranslation([ 'environment', 'settings', 'common' ])
  const dispatch = useAppDispatch()
  const confirm = useConfirm()
  const variables = useAppSelector(selectAllEnvironmentVariables)
  const status = useEnvironmentVariablesLoader()
  const navigate = useNavigate()

  function confirmDelete(variable: EnvironmentVariable) {
    confirm({
      title: t('delete.title', { key: variable.key }),
      message: t('delete.body'),
      tone: 'danger',
      confirmText: t('common:actions.delete'),
      onConfirm: async () => {
        try {
          await deleteEnvironmentVariable(variable.id)
        }
        catch (error) {
          console.debug('ManageEnvironmentPage failed to delete a variable', { error, variableId: variable.id })
          toast.danger(t('common:errors.unexpected'))
          // Keeps the dialog open to try again.
          throw error
        }

        dispatch(environmentVariableDeleted(variable.id))
        toast.success(t('toasts.deleted', { key: variable.key }))
      },
    })
  }

  return <div className='container'>
    <Breadcrumbs className='compact'>
      <Breadcrumbs.Item href={UrlTree.settings}>{t('settings:title')}</Breadcrumbs.Item>
      <Breadcrumbs.Item>{t('title')}</Breadcrumbs.Item>
    </Breadcrumbs>
    <section>
      <div className='level compact items-center'>
        <h1 className='text-3xl font-bold'>{
          t('title')
        }</h1>
        <Button
          size='sm'
          variant='outline'
          className='shrink-0'
          onPress={() => navigate(UrlTree.settingsEnvironmentNew)}
        >
          <LuPlus className='size-4' aria-hidden />
          <span>{t('variables.add')}</span>
        </Button>
      </div>
      <p className='compact text-sm opacity-70'>{
        t('description')
      }</p>
      <div className='relaxed'>
        <EnvironmentVisibilityNote />
      </div>

      {status === 'loading' && <div className='grid place-items-center py-16'>
        <Spinner />
      </div>}

      {status === 'failed' && <p className='py-10 text-center text-sm text-danger'>{
        t('table.loadError')
      }</p>}

      {status === 'loaded' && <EnvironmentVariableTable
        variables={variables}
        onEdit={(variable) => navigate(getEnvironmentVariableEditUrl(variable.id))}
        onDelete={confirmDelete}
      />}
    </section>
  </div>
}
