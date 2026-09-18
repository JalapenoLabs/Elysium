// Copyright © 2026 Jalapeno Labs

import type {
  JiraBoardSelection,
  JiraCredential,
  JiraProjectSelection,
} from '../../../api/routes/jiraRoutes'
import type { JiraFormInput, JiraFormValues } from './jiraFormSchema'
import type { JiraScopeSelection } from './jiraPresentation'

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
import {
  findMissingSelectionNames,
  keepsStoredJiraToken,
  normalizeJiraSiteUrl,
  toJiraScopeSelection,
} from './jiraPresentation'

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

  const [ scope, setScope ] = useState<JiraScopeSelection>(() => toJiraScopeSelection(stored))
  // A rename must not carry an allowlist: the API re-checks every one it is sent, and a
  // stored project the token has since lost would refuse the save.
  const [ isScopeChanged, setIsScopeChanged ] = useState(false)
  const [ failureMessage, setFailureMessage ] = useState<string | null>(null)

  const projects = useSWR(
    [ 'jira-credential-projects', stored.id ],
    ([ , credentialId ]) => listJiraCredentialProjects(credentialId),
  )
  const boards = useSWR(
    [ 'jira-credential-boards', stored.id ],
    ([ , credentialId ]) => listJiraCredentialBoards(credentialId),
  )

  const onSubmit = form.handleSubmit(async (values) => {
    setFailureMessage(null)
    try {
      const response = await updateJiraCredential(stored.id, {
        name: values.name,
        siteUrl: values.siteUrl,
        accountEmail: values.accountEmail,
        // A blank token means "keep the stored one", so it is only sent when typed.
        token: values.token || undefined,
        ...scopeRequest(),
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

      // A 400 names what to check, and it can be the token, the site, or a project the
      // token lost since this page opened, so it sits above the whole form.
      const message = getApiErrorMessage(error)
      if (!message) {
        console.debug('JiraCredentialForm failed to save the credential', { error })
      }
      setFailureMessage(message ?? t('common:errors.unexpected'))
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
  const missing = findMissingSelectionNames(stored, reachableProjects, reachableBoards)
  const hasScopeFailed = Boolean(projects.error || boards.error)
  const isLoadingScope = !hasScopeFailed && (!projects.data || !boards.data)
  // Everything is missing before the lists arrive, so the warnings wait for them, and say
  // nothing about the old site once another one is typed.
  const isScopeReady = !isLoadingScope && !hasScopeFailed && !isMovingSite
  // A selection missing from a whole listing is one the token lost. Missing from a listing
  // Jira cut short, it may be one the listing never reached, and the two cannot be told
  // apart, so that list alone is neither narrowed nor reported as lost. Each list is judged
  // on its own truncation: a site with more boards than Elysium reads says nothing about
  // whether its projects are whole.
  const isProjectScopeLocked = isScopeReady
    && (projects.data?.truncated ?? false)
    && missing.projects.length > 0
  const isBoardScopeLocked = isScopeReady
    && (boards.data?.truncated ?? false)
    && missing.boards.length > 0
  const lostNames = [
    ...(isProjectScopeLocked ? [] : missing.projects),
    ...(isBoardScopeLocked ? [] : missing.boards),
  ]
  const beyondNames = [
    ...(isProjectScopeLocked ? missing.projects : []),
    ...(isBoardScopeLocked ? missing.boards : []),
  ]
  const hasLostAccess = isScopeReady && lostNames.length > 0

  // The allowlist half of the request. Moving site must bring both, and the old site's
  // ids name nothing there, so it starts open again; an untouched allowlist is left out
  // altogether, since the API checks with Jira every one it is sent; and what is sent
  // holds only what Jira reports today, which is all the API will accept.
  function scopeRequest(): { projects?: JiraProjectSelection, boards?: JiraBoardSelection } {
    if (isMovingSite) {
      return { projects: ALL_JIRA_ITEMS, boards: ALL_JIRA_ITEMS }
    }
    if (!isScopeChanged) {
      return {}
    }

    const reachableProjectIds = new Set(reachableProjects.map((project) => project.id))
    const reachableBoardIds = new Set(reachableBoards.map((board) => board.id))
    const request: { projects?: JiraProjectSelection, boards?: JiraBoardSelection } = {}

    // A locked list is left out of the request altogether, so it stays exactly as it is.
    // Sending it would drop the selections this listing never reached, and the API could
    // not keep them either, since its own listing stops at the same cap.
    if (isProjectScopeLocked) {
      console.debug('JiraCredentialForm left the projects allowlist alone', {
        credentialId: stored.id,
        missing: missing.projects,
      })
    }
    else {
      request.projects = scope.allProjects
        ? ALL_JIRA_ITEMS
        : scope.projectIds.filter((id) => reachableProjectIds.has(id))
    }

    if (isBoardScopeLocked) {
      console.debug('JiraCredentialForm left the boards allowlist alone', {
        credentialId: stored.id,
        missing: missing.boards,
      })
    }
    else {
      request.boards = scope.allBoards
        ? ALL_JIRA_ITEMS
        : scope.boardIds.filter((id) => reachableBoardIds.has(id))
    }

    return request
  }

  return <div className='grid grid-cols-1 items-start gap-8 lg:grid-cols-[minmax(0,1fr)_minmax(0,24rem)]'>
    <Form onSubmit={onSubmit} validationBehavior='aria' className='flex flex-col gap-4'>
      <JiraConnectionFields
        form={form}
        keepsStoredToken={keepsStoredToken}
        isEditing
      />

      <p className='text-sm opacity-70'>{t('form.checkedHint')}</p>

      {hasLostAccess && <Alert status='warning'>
        <Alert.Indicator />
        <Alert.Content>
          <Alert.Description>{
            t('scope.unreachable', {
              count: lostNames.length,
              names: lostNames.join(', '),
            })
          }</Alert.Description>
        </Alert.Content>
      </Alert>}

      {Boolean(beyondNames.length) && <Alert status='warning'>
        <Alert.Indicator />
        <Alert.Content>
          <Alert.Description>{
            t('scope.beyondListing', {
              count: beyondNames.length,
              names: beyondNames.join(', '),
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
        projectsTruncated={projects.data?.truncated ?? false}
        boardsTruncated={boards.data?.truncated ?? false}
        value={scope}
        onChange={(next) => {
          setScope(next)
          setIsScopeChanged(true)
        }}
        isProjectsDisabled={isMovingSite || isProjectScopeLocked}
        isBoardsDisabled={isMovingSite || isBoardScopeLocked}
      />}

      {failureMessage && <Alert status='danger'>
        <Alert.Indicator />
        <Alert.Content>
          <Alert.Description>{failureMessage}</Alert.Description>
        </Alert.Content>
      </Alert>}

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
