// Copyright © 2026 Jalapeno Labs

// Core
import { useNavigate } from 'react-router'
import { useTranslation } from 'react-i18next'

// User interface
import { EnvironmentVariableEditorLayout } from './EnvironmentVariableEditorLayout'
import { EnvironmentVariableForm } from './EnvironmentVariableForm'

// Misc
import { UrlTree } from '../../../urls'

// `/settings/environment/new`: adding an environment variable.
export function AddEnvironmentVariablePage() {
  const { t } = useTranslation('environment')
  const navigate = useNavigate()

  return <EnvironmentVariableEditorLayout
    title={t('form.createTitle')}
  >
    <EnvironmentVariableForm
      variable={null}
      onSaved={() => navigate(UrlTree.settingsEnvironment)}
      onCancel={() => navigate(UrlTree.settingsEnvironment)}
    />
  </EnvironmentVariableEditorLayout>
}
