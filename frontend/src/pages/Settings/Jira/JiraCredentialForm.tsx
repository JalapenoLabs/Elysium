// Copyright © 2026 Jalapeno Labs

import type { JiraCredential } from '../../../api/routes/jiraRoutes'
import type { JiraFormInput, JiraFormValues } from './jiraFormSchema'
import type { JiraScopeSelection } from './JiraScopeFields'

// Core
import { useMemo, useState } from 'react'
import { useForm, useWatch } from 'react-hook-form'
import { useTranslation } from 'react-i18next'
import useSWR from 'swr'

// Redux
import { useAppDispatch } from '../../../store/hooks'
import { jiraCredentialUpserted } from '../../../store/jiraCredentialsSlice'

// User interface
import { Alert, Button, Form, Spinner, toast } from '@heroui/react'
import { JiraConnectionFields } from './JiraConnectionFields'
import { JiraScopeFields } from './JiraScopeFields'
import { JiraSetupChecklist } from './JiraSetupChecklist'

// Utility
import { zodResolver } from '@hookform/resolvers/zod'
import { HTTPError } from 'ky'

// Misc
import { getApiErrorMessage } from '../../../api/errors'
import {
  ALL_JIRA_ITEMS,
  listJiraCredentialBoards,
  listJiraCredentialProjects,
  updateJiraCredential,
} from '../../../api/routes/jiraRoutes'
import { createJiraFormSchema } from './jiraFormSchema'
import { findUnreachableNames, keepsStoredJiraToken, normalizeJiraSiteUrl } from './jiraPresentation'

type Props = {
  credential: JiraCredential
  onSaved: () => void
  onCancel: () => void
}

// Edits a stored Jira site. The token is write-only: a blank field keeps the stored one,
// which only holds while the site and account it was created for are unchanged. What the
// token reaches is loaded fresh, so the pickers offer today's projects and boards rather
// than the ones the credential was saved with.
export function JiraCredentialForm(props: Props) {
  const { t } = useTranslation([ 'jira', 'common' ])
  const dispatch = useAppDispatch()
  const stored = props.credential

  const resolver = useMemo(() => zodResolver(createJiraFormSchema(t, stored)), [ t, stored ])

  const form = useForm<JiraFormInput, unknown, JiraFormValues>({
    resolver,
    defaultValues: {
      name: stored.name,
      siteUrl: stored.siteUrl,
      accountEmail: stored.accountEmail,
      token: '',
    },
  })

  const [ scope, setScope ] = useState<JiraScopeSelection>({
    allProjects: stored.allProjects,
    projectIds: stored.projects.map((project) => project.projectId),
    allBoards: stored.allBoards,
    boardIds: stored.boards.map((board) => board.boardId),
  })

  const projects = useSWR(
    [ 'jira-credential-projects', stored.id ],
    ([ , credentialId ]) => listJiraCredentialProjects(credentialId),
  )
  const boards = useSWR(
    [ 'jira-credential-boards', stored.id ],
    ([ , credentialId ]) => listJiraCredentialBoards(credentialId),
  )

  const onSubmit = form.handleSubmit(async (values) => {
    try {
      const response = await updateJiraCredential(stored.id, {
        name: values.name,
        siteUrl: values.siteUrl,
        accountEmail: values.accountEmail,
        // A blank token means "keep the stored one", so it is only sent when typed.
        token: values.token || undefined,
        projects: scope.allProjects
          ? ALL_JIRA_ITEMS
          : scope.projectIds,
        boards: scope.allBoards
          ? ALL_JIRA_ITEMS
          : scope.boardIds,
      })
      dispatch(jiraCredentialUpserted(response.credential))
      toast.success(t('toasts.updated', { name: values.name }))
      props.onSaved()
    }
    catch (error) {
      if (error instanceof HTTPError && error.response.status === 409) {
        form.setError('name', { message: t('form.errors.nameTaken') })
        return
      }

      // A token Jira refuses comes back as a 400 naming what to check, which belongs on
      // the field the user can fix.
      const message = getApiErrorMessage(error)
      if (error instanceof HTTPError && error.response.status === 400 && message) {
        form.setError('token', { message })
        return
      }

      console.debug('JiraCredentialForm failed to save the credential', { error })
      toast.danger(t('common:errors.unexpected'), { description: message ?? undefined })
    }
  })

  // useWatch subscribes per field and, unlike form.watch, is safe for the React Compiler.
  const values = useWatch({ control: form.control })
  const keepsStoredToken = keepsStoredJiraToken(stored, values.siteUrl ?? '', values.accountEmail ?? '')

  // The loaded lists belong to the stored site. Once another site is typed, a project id
  // means nothing there, so the allowlist waits for the new site to be saved.
  const isMovingSite = normalizeJiraSiteUrl(values.siteUrl ?? '') !== stored.siteUrl
  const reachableProjects = projects.data?.projects ?? []
  const reachableBoards = boards.data?.boards ?? []
  const unreachableNames = findUnreachableNames(stored, reachableProjects, reachableBoards)
  const hasScopeFailed = Boolean(projects.error || boards.error)
  const isLoadingScope = !hasScopeFailed && (!projects.data || !boards.data)

  return <div className='grid grid-cols-1 items-start gap-8 lg:grid-cols-[minmax(0,1fr)_minmax(0,24rem)]'>
    <Form onSubmit={onSubmit} validationBehavior='aria' className='flex flex-col gap-4'>
      <JiraConnectionFields
        form={form}
        keepsStoredToken={keepsStoredToken}
        isEditing
      />

      <p className='text-sm opacity-70'>{t('form.checkedHint')}</p>

      {unreachableNames.length > 0 && <Alert status='warning'>
        <Alert.Indicator />
        <Alert.Content>
          <Alert.Description>{
            t('scope.unreachable', {
              count: unreachableNames.length,
              names: unreachableNames.join(', '),
            })
          }</Alert.Description>
        </Alert.Content>
      </Alert>}

      {isMovingSite && <p className='text-sm opacity-70'>{t('scope.siteChanged')}</p>}

      {isLoadingScope && <div className='grid place-items-center py-8'>
        <Spinner />
      </div>}

      {hasScopeFailed && <p className='text-sm text-danger'>{t('scope.loadError')}</p>}

      {!isLoadingScope && !hasScopeFailed && <JiraScopeFields
        projectOptions={reachableProjects}
        boardOptions={reachableBoards}
        value={scope}
        onChange={setScope}
        isDisabled={isMovingSite}
      />}

      <div className='mt-2 flex justify-end gap-2'>
        <Button variant='tertiary' onPress={props.onCancel}>
          <span>{t('common:actions.cancel')}</span>
        </Button>
        <Button
          type='submit'
          isPending={form.formState.isSubmitting}
        >
          <span>{t('common:actions.save')}</span>
        </Button>
      </div>
    </Form>

    <aside className='lg:sticky lg:top-4'>
      <JiraSetupChecklist />
    </aside>
  </div>
}
