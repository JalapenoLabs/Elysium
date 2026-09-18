// Copyright © 2026 Jalapeno Labs

import type { JiraDiscovery } from '../../../api/routes/jiraRoutes'
import type { JiraFormInput, JiraFormValues } from './jiraFormSchema'

// Core
import { useMemo, useState } from 'react'
import { useForm } from 'react-hook-form'
import { useTranslation } from 'react-i18next'

// User interface
import { Alert, Button, Form } from '@heroui/react'
import { JiraConnectionFields } from './JiraConnectionFields'
import { JiraSetupChecklist } from './JiraSetupChecklist'

// Utility
import { zodResolver } from '@hookform/resolvers/zod'

// Misc
import { getApiErrorMessage } from '../../../api/errors'
import { discoverJira } from '../../../api/routes/jiraRoutes'
import { createJiraFormSchema } from './jiraFormSchema'

type Props = {
  // What was typed before, so stepping back from the second step keeps it.
  defaultValues: JiraFormInput
  onDiscovered: (connection: JiraFormValues, discovery: JiraDiscovery) => void
  onCancel: () => void
}

// The first step of adding a Jira site: who Elysium signs in as, and where. Submitting
// asks Jira what the token reaches and hands the answer up. Nothing is stored yet, so a
// site that turns out to be wrong costs only a second attempt.
export function JiraConnectionForm(props: Props) {
  const { t } = useTranslation([ 'jira', 'common' ])
  const [ failureMessage, setFailureMessage ] = useState<string | null>(null)

  const resolver = useMemo(() => zodResolver(createJiraFormSchema(t, null)), [ t ])

  const form = useForm<JiraFormInput, unknown, JiraFormValues>({
    resolver,
    defaultValues: props.defaultValues,
  })

  const onSubmit = form.handleSubmit(async (values) => {
    setFailureMessage(null)
    try {
      const discovery = await discoverJira({
        siteUrl: values.siteUrl,
        accountEmail: values.accountEmail,
        token: values.token,
      })
      props.onDiscovered(values, discovery)
    }
    catch (error) {
      // A refused token or an unknown site answers 400, and an unreachable Jira 502. The
      // message names which of the four fields to look at, so it belongs above them all
      // rather than pinned to one.
      const message = getApiErrorMessage(error)
      if (!message) {
        console.debug('JiraConnectionForm could not reach Jira', { error })
      }
      setFailureMessage(message ?? t('common:errors.unexpected'))
    }
  })

  return <div className='grid grid-cols-1 items-start gap-8 lg:grid-cols-[minmax(0,1fr)_minmax(0,24rem)]'>
    <Form onSubmit={onSubmit} validationBehavior='aria' className='flex flex-col gap-4'>
      <JiraConnectionFields
        form={form}
        keepsStoredToken={false}
        isEditing={false}
      />

      <p className='text-sm opacity-70'>{t('steps.connectHint')}</p>

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
          <span>{t('steps.continue')}</span>
        </Button>
      </div>
    </Form>

    <aside className='lg:sticky lg:top-4'>
      <JiraSetupChecklist />
    </aside>
  </div>
}
