// Copyright © 2026 Jalapeno Labs

// Core
import { useTranslation } from 'react-i18next'

// User interface
import { SetupChecklist } from '../../../components/SetupChecklist'

// Misc
import { JIRA_SETUP_STEPS } from './jiraPresentation'

// Steps for creating an Atlassian API token, in Atlassian's own words.
export function JiraSetupChecklist() {
  const { t } = useTranslation('jira')

  return <SetupChecklist
    title={t('setup.title')}
    description={t('setup.description')}
    steps={JIRA_SETUP_STEPS.map((step) => ({
      ...step,
      id: step.textKey,
      text: t(step.textKey),
    }))}
  />
}
