// Copyright © 2026 Jalapeno Labs

import type { EnvironmentVariable } from '../../../api/routes/environmentRoutes'

// Core
import { useTranslation } from 'react-i18next'

// Redux
import { useAppDispatch, useAppSelector } from '../../../store/hooks'
import { environmentVariableDeleted, selectAllEnvironmentVariables } from '../../../store/environmentVariablesSlice'

// User interface
import { buttonVariants, Link, Spinner } from '@heroui/react'
import { LuPlus } from 'react-icons/lu'
import { SettingsPage } from '../SettingsPage'
import { EnvironmentVariableTable } from './EnvironmentVariableTable'

// Misc
import { deleteEnvironmentVariable } from '../../../api/routes/environmentRoutes'
import { useConfirm } from '../../../hooks/useConfirm'
import { useEnvironmentVariablesLoader } from '../../../hooks/useServerData'
import { UrlTree } from '../../../urls'
import { toast } from '../../../toast/toast'

// `/settings/environment`: the global environment variables every coding session receives.
export function ManageEnvironmentPage() {
  const { t } = useTranslation([ 'environment', 'common' ])
  const dispatch = useAppDispatch()
  const confirm = useConfirm()
  const variables = useAppSelector(selectAllEnvironmentVariables)
  const status = useEnvironmentVariablesLoader()

  function confirmDelete(variable: EnvironmentVariable) {
    confirm({
      title: t('delete.title', { key: variable.key }),
      message: t('delete.body', { key: variable.key }),
      isIrreversible: true,
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

  return <SettingsPage
    title={t('title')}
    help={t('help')}
    primaryAction={<Link href={UrlTree.settingsEnvironmentNew} className={buttonVariants({ variant: 'outline' })}>
      <LuPlus className='size-4' aria-hidden />
      <span>{t('variables.add')}</span>
    </Link>}
  >
    <section>
      {status === 'loading' && <div className='grid place-items-center py-16'>
        <Spinner />
      </div>}

      {status === 'failed' && <p className='py-10 text-center text-sm text-danger'>{
        t('table.loadError')
      }</p>}

      {status === 'loaded' && <EnvironmentVariableTable
        variables={variables}
        onDelete={confirmDelete}
      />}
    </section>
  </SettingsPage>
}
