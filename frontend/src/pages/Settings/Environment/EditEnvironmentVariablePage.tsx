// Copyright © 2026 Jalapeno Labs

// Core
import { useNavigate, useParams } from 'react-router'
import { useTranslation } from 'react-i18next'

// Redux
import { useAppSelector } from '../../../store/hooks'
import { selectEnvironmentVariableById } from '../../../store/environmentVariablesSlice'

// User interface
import { Link, Spinner } from '@heroui/react'
import { EnvironmentVariableEditorLayout } from './EnvironmentVariableEditorLayout'
import { EnvironmentVariableForm } from './EnvironmentVariableForm'

// Misc
import { useEnvironmentVariablesLoader } from '../../../hooks/useServerData'
import { UrlTree } from '../../../urls'

// `/settings/environment/:variableId/edit`: editing one environment variable.
export function EditEnvironmentVariablePage() {
  const { t } = useTranslation('environment')
  const navigate = useNavigate()
  const { variableId = '' } = useParams()
  const status = useEnvironmentVariablesLoader()
  const variable = useAppSelector((state) => selectEnvironmentVariableById(state, variableId))

  if (!variable) {
    if (status === 'loading') {
      return <div className='grid place-items-center py-16'>
        <Spinner />
      </div>
    }
    return <div className='container py-10 text-center text-sm'>
      <p className='compact opacity-70'>{t('page.notFound')}</p>
      <Link href={UrlTree.settingsEnvironment} className='text-link'>{t('page.backToList')}</Link>
    </div>
  }

  return <EnvironmentVariableEditorLayout
    title={t('form.editTitle', { key: variable.key })}
  >
    <EnvironmentVariableForm
      // Remount when navigating between variables so the form takes the new values.
      key={variable.id}
      variable={variable}
      onSaved={() => navigate(UrlTree.settingsEnvironment)}
      onCancel={() => navigate(UrlTree.settingsEnvironment)}
    />
  </EnvironmentVariableEditorLayout>
}
