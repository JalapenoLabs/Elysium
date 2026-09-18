// Copyright © 2026 Jalapeno Labs

import type { JiraDiscovery } from '../../../api/routes/jiraRoutes'
import type { JiraFormInput, JiraFormValues } from './jiraFormSchema'

// Core
import { useState } from 'react'
import { useNavigate } from 'react-router'
import { useTranslation } from 'react-i18next'

// User interface
import { LuChevronRight } from 'react-icons/lu'
import { JiraConnectionForm } from './JiraConnectionForm'
import { JiraCredentialEditorLayout } from './JiraCredentialEditorLayout'
import { JiraScopeForm } from './JiraScopeForm'

// Misc
import { UrlTree } from '../../../urls'

const EMPTY_CONNECTION: JiraFormInput = {
  name: '',
  siteUrl: '',
  accountEmail: '',
  token: '',
}

// Adding a site takes two steps, and which step it is decides what is known: the second
// only exists once Jira has answered, and stepping back carries what was typed with it.
type CreateState =
  | { step: 'connect', typed: JiraFormInput }
  | { step: 'choose', connection: JiraFormValues, discovery: JiraDiscovery }

// `/settings/jira/new`: adding a Jira site. The token is checked with Jira before
// anything is stored, so the projects and boards on offer are the ones it really reaches.
export function AddJiraCredentialPage() {
  const { t } = useTranslation('jira')
  const navigate = useNavigate()
  const [ state, setState ] = useState<CreateState>({ step: 'connect', typed: EMPTY_CONNECTION })

  return <JiraCredentialEditorLayout
    title={t('form.createTitle')}
  >
    <ol className='relaxed flex items-center gap-3 text-sm'>
      <li className={state.step === 'connect'
        ? 'flex items-center gap-2 font-medium'
        : 'flex items-center gap-2 opacity-60'}
      >
        <span className='opacity-60'>1</span>
        <span>{t('steps.connect')}</span>
      </li>
      <LuChevronRight className='size-4 opacity-40' aria-hidden />
      <li className={state.step === 'choose'
        ? 'flex items-center gap-2 font-medium'
        : 'flex items-center gap-2 opacity-60'}
      >
        <span className='opacity-60'>2</span>
        <span>{t('steps.choose')}</span>
      </li>
    </ol>

    {state.step === 'connect'
      ? <JiraConnectionForm
        defaultValues={state.typed}
        onDiscovered={(connection, discovery) => setState({ step: 'choose', connection, discovery })}
        onCancel={() => navigate(UrlTree.settingsJira)}
      />
      : <JiraScopeForm
        connection={state.connection}
        discovery={state.discovery}
        onBack={() => setState({ step: 'connect', typed: state.connection })}
        onSaved={() => navigate(UrlTree.settingsJira)}
      />}
  </JiraCredentialEditorLayout>
}
