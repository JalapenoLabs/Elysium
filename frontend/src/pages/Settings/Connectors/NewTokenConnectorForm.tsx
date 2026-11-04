// Copyright © 2026 Jalapeno Labs

import type { FormEvent } from 'react'
import type {
  AccountDefinition,
  ConnectorKindDefinition,
} from '../../../api/routes/connectorRoutes'

// Core
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { useNavigate } from 'react-router'

// Redux
import { checkRunLoaded, connectorUpserted, selectCheckRun } from '../../../store/connectorsSlice'
import { useAppDispatch, useAppSelector } from '../../../store/hooks'

// User interface
import { Button, Card, Form, toast } from '@heroui/react'
import { AccountFields } from './AccountFields'
import { CheckList } from './CheckList'
import { ContentFields } from './ContentFields'

// Misc
import { checkConnectorDraft, createConnector } from '../../../api/routes/connectorRoutes'
import { getConnectorUrl } from '../../../urls'
import { useSettingsSave } from '../settingsSave'
import { connectorErrorMessage, hasRequiredFailure } from './connectorPresentation'

const NEW_CONNECTOR_FORM_ID = 'new-connector-form'

type Props = {
  definition: ConnectorKindDefinition
  accountDefinition: AccountDefinition
}

// A new connector that signs in with a token: the account it owns, what it reads, and its
// checks, run against what was typed before anything is stored. Create waits for a finished
// run, since it is what proves the token works, and offers "Create anyway" when a required
// check failed. Changing anything after a run asks for another.
export function NewTokenConnectorForm(props: Props) {
  const { t } = useTranslation([ 'connectors', 'common' ])
  const dispatch = useAppDispatch()
  const navigate = useNavigate()
  const [ fields, setFields ] = useState<Record<string, string>>({})
  const [ content, setContent ] = useState<Record<string, unknown>>({})
  const [ runId, setRunId ] = useState<string | null>(null)
  const [ isStartingChecks, setIsStartingChecks ] = useState(false)
  const [ isSaving, setIsSaving ] = useState(false)
  const run = useAppSelector((state) => selectCheckRun(state, runId))

  const kind = props.definition.kind
  const account = { new: fields }
  const isDirty = Object.values(fields).some(Boolean) || Object.keys(content).length > 0
  const isFinished = Boolean(run?.finishedAt)
  const failedRequired = run
    ? hasRequiredFailure(props.definition.checks, run.checks)
    : false

  useSettingsSave({
    formId: NEW_CONNECTOR_FORM_ID,
    isDirty,
    isPending: isSaving,
    blockedReason: isFinished
      ? null
      : t('form.checkFirst'),
    label: failedRequired
      ? t('form.createAnyway')
      : t('form.create'),
  })

  async function runChecks() {
    setIsStartingChecks(true)
    try {
      const response = await checkConnectorDraft({ kind, account, content })
      dispatch(checkRunLoaded(response.run))
      setRunId(response.run.id)
    }
    catch (error) {
      console.debug('NewTokenConnectorForm failed to start checks', { error, kind })
      toast.danger(connectorErrorMessage(error, t))
    }
    finally {
      setIsStartingChecks(false)
    }
  }

  async function create() {
    setIsSaving(true)
    try {
      const response = await createConnector({ kind, account, content })
      dispatch(connectorUpserted(response.connector))
      toast.success(t('form.created'))
      void navigate(getConnectorUrl(response.connector.id))
    }
    catch (error) {
      console.debug('NewTokenConnectorForm failed to create the connector', { error, kind })
      toast.danger(connectorErrorMessage(error, t))
      setIsSaving(false)
    }
  }

  // Save, Enter, and Ctrl+S submit the form only once Save can be pressed, after a finished run.
  function onSubmit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    void create()
  }

  return <Form id={NEW_CONNECTOR_FORM_ID} onSubmit={onSubmit} validationBehavior='aria'>
    <Card className='relaxed w-full p-6'>
      <h2 className='compact font-semibold'>{t('form.signIn')}</h2>
      <AccountFields
        definition={props.accountDefinition}
        values={fields}
        onChange={(values) => {
          setFields(values)
          setRunId(null)
        }}
      />
    </Card>

    {Boolean(props.definition.content.length) && <Card className='relaxed w-full p-6'>
      <h2 className='compact font-semibold'>{t('detail.content')}</h2>
      <ContentFields
        kind={kind}
        fields={props.definition.content}
        account={account}
        value={content}
        onChange={(value) => {
          setContent(value)
          setRunId(null)
        }}
      />
    </Card>}

    <Card className='relaxed w-full p-6'>
      <div className='level items-start gap-4'>
        <h2 className='font-semibold'>{t('form.testConnection')}</h2>
        <Button
          variant='outline'
          isDisabled={!isDirty}
          isPending={isStartingChecks || Boolean(run && !run.finishedAt)}
          onPress={() => void runChecks()}
        >
          <span>{t('form.runChecks')}</span>
        </Button>
      </div>
      {failedRequired && <p className='mt-2 text-sm text-warning'>{t('form.requiredFailed')}</p>}
    </Card>

    {run && <CheckList
      kind={kind}
      specs={props.definition.checks}
      outcomes={run.checks}
      reportSubjectOf={(checkId) => ({ kind: 'checkRun', runId: run.id, checkId })}
    />}
  </Form>
}
