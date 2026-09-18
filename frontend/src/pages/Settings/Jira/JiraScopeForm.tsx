// Copyright © 2026 Jalapeno Labs

import type { FormEvent } from 'react'
import type { JiraDiscovery } from '../../../api/routes/jiraRoutes'
import type { JiraFormValues } from './jiraFormSchema'
import type { JiraScopeSelection } from './JiraScopeFields'

// Core
import { useState } from 'react'
import { useTranslation } from 'react-i18next'

// Redux
import { useAppDispatch } from '../../../store/hooks'
import { jiraCredentialUpserted } from '../../../store/jiraCredentialsSlice'

// User interface
import { Alert, Button, Card, Form, toast } from '@heroui/react'
import { JiraScopeFields } from './JiraScopeFields'

// Misc
import { getApiErrorMessage } from '../../../api/errors'
import { ALL_JIRA_ITEMS, createJiraCredential } from '../../../api/routes/jiraRoutes'

type Props = {
  // What the first step was signed in with, token included: the credential is stored in
  // this step, so the token travels no further than this request.
  connection: JiraFormValues
  discovery: JiraDiscovery
  onBack: () => void
  onSaved: () => void
}

// The second step of adding a Jira site: which of the projects and boards Jira reported
// Elysium may read. Everything is allowed until it is narrowed, since a token that only
// reaches what its owner reaches is already scoped.
export function JiraScopeForm(props: Props) {
  const { t } = useTranslation([ 'jira', 'common' ])
  const dispatch = useAppDispatch()
  const [ failureMessage, setFailureMessage ] = useState<string | null>(null)
  const [ isSubmitting, setIsSubmitting ] = useState(false)
  const [ scope, setScope ] = useState<JiraScopeSelection>({
    allProjects: true,
    projectIds: [],
    allBoards: true,
    boardIds: [],
  })

  async function onSubmit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    setFailureMessage(null)
    setIsSubmitting(true)

    try {
      const response = await createJiraCredential({
        name: props.connection.name,
        siteUrl: props.connection.siteUrl,
        accountEmail: props.connection.accountEmail,
        token: props.connection.token,
        projects: scope.allProjects
          ? ALL_JIRA_ITEMS
          : scope.projectIds,
        boards: scope.allBoards
          ? ALL_JIRA_ITEMS
          : scope.boardIds,
      })
      dispatch(jiraCredentialUpserted(response.credential))
      toast.success(t('toasts.created', { name: props.connection.name }))
      props.onSaved()
    }
    catch (error) {
      // A name already taken, a token revoked between the two steps, or an unreachable
      // Jira. Every one of them is fixed a step back, so the message sits above Back.
      const message = getApiErrorMessage(error)
      if (!message) {
        console.debug('JiraScopeForm failed to create the credential', { error })
      }
      setFailureMessage(message ?? t('common:errors.unexpected'))
    }
    finally {
      setIsSubmitting(false)
    }
  }

  return <Form onSubmit={onSubmit} validationBehavior='aria' className='flex flex-col gap-4'>
    <Card>
      <Card.Header>
        <Card.Title>{props.connection.name}</Card.Title>
        <Card.Description>{
          t('steps.connectedAs', {
            name: props.discovery.account.displayName,
            email: props.discovery.account.email,
          })
        }</Card.Description>
      </Card.Header>
      <Card.Content className='flex flex-col gap-1 text-sm opacity-70'>
        <span>{props.connection.siteUrl}</span>
        <span>{t('steps.reachable', { count: props.discovery.projects.length })}</span>
        <span>{t('steps.reachableBoards', { count: props.discovery.boards.length })}</span>
      </Card.Content>
    </Card>

    <p className='text-sm opacity-70'>{t('steps.chooseHint')}</p>

    <JiraScopeFields
      projectOptions={props.discovery.projects}
      boardOptions={props.discovery.boards}
      value={scope}
      onChange={setScope}
      isDisabled={isSubmitting}
    />

    {failureMessage && <Alert status='danger'>
      <Alert.Indicator />
      <Alert.Content>
        <Alert.Description>{failureMessage}</Alert.Description>
      </Alert.Content>
    </Alert>}

    <div className='mt-2 flex justify-end gap-2'>
      <Button variant='tertiary' onPress={props.onBack}>
        <span>{t('steps.back')}</span>
      </Button>
      <Button type='submit' isPending={isSubmitting}>
        <span>{t('common:actions.save')}</span>
      </Button>
    </div>
  </Form>
}
