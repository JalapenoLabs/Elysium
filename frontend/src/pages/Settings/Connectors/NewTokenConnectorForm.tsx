// Copyright © 2026 Jalapeno Labs

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
import { Button, Card, Form } from '@heroui/react'
import { AccountFields } from './AccountFields'
import { CheckList } from './CheckList'
import { ContentFields } from './ContentFields'
import { PageCreateFooter } from '../../../layout/page/PageCreateFooter'

// Misc
import { checkConnectorDraft, createConnector } from '../../../api/routes/connectorRoutes'
import { forgetSessionDraft, keepSessionDraft, readSessionDraft } from '../../../drafts/sessionDrafts'
import { getConnectorUrl } from '../../../urls'
import { usePageSave } from '../../../layout/page/pageSave'
import { connectorErrorMessage, hasRequiredFailure } from './connectorPresentation'
import { toast } from '../../../toast/toast'

type Props = {
  definition: ConnectorKindDefinition
  accountDefinition: AccountDefinition
}

// The account's fields worth keeping as a draft: every one typed in but a secret, which is
// never written to storage. Read back the same way, since storage is outside the app.
function publicAccountFields(values: unknown, definition: AccountDefinition) {
  const kept: Record<string, string> = {}
  if (!values || typeof values !== 'object') {
    return kept
  }
  for (const field of definition.fields) {
    const value: unknown = Reflect.get(values, field.id)
    if (field.field.type !== 'secret' && typeof value === 'string' && value) {
      kept[field.id] = value
    }
  }
  return kept
}

// A new connector that signs in with a token: the account it owns, what it reads, and its
// checks, run against what was typed before anything is stored. Create waits for a finished
// run, since it is what proves the token works, and offers "Create anyway" when a required
// check failed. Changing anything after a run asks for another.
export function NewTokenConnectorForm(props: Props) {
  const { t } = useTranslation([ 'connectors', 'common' ])
  const dispatch = useAppDispatch()
  const navigate = useNavigate()
  // What is typed, but the token, is kept for the tab until the connector is created.
  const draftName = `connector.${props.definition.kind}`
  const [ fields, setFields ] = useState(
    () => publicAccountFields(readSessionDraft(draftName), props.accountDefinition),
  )
  const [ content, setContent ] = useState<Record<string, unknown>>({})
  const [ runId, setRunId ] = useState<string | null>(null)
  const [ isStartingChecks, setIsStartingChecks ] = useState(false)
  const run = useAppSelector((state) => selectCheckRun(state, runId))

  const kind = props.definition.kind
  const account = { new: fields }
  const isDirty = Object.values(fields).some(Boolean) || Object.keys(content).length > 0
  const isFinished = Boolean(run?.finishedAt)
  const failedRequired = run
    ? hasRequiredFailure(props.definition.checks, run.checks)
    : false

  // Save, Enter, and Ctrl+S create only once Save can be pressed, after a finished run.
  const onSubmit = usePageSave({
    save: create,
    isDirty,
    onDiscard: () => forgetSessionDraft(draftName),
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
    try {
      const response = await createConnector({ kind, account, content })
      dispatch(connectorUpserted(response.connector))
      forgetSessionDraft(draftName)
      toast.success(t('form.created'))
      void navigate(getConnectorUrl(response.connector.id))
      return true
    }
    catch (error) {
      console.debug('NewTokenConnectorForm failed to create the connector', { error, kind })
      toast.danger(connectorErrorMessage(error, t))
      return false
    }
  }

  return <Form onSubmit={onSubmit} validationBehavior='aria'>
    <Card className='relaxed w-full p-6'>
      <h2 className='compact font-semibold'>{t('form.signIn')}</h2>
      <AccountFields
        definition={props.accountDefinition}
        values={fields}
        onChange={(values) => {
          setFields(values)
          const kept = publicAccountFields(values, props.accountDefinition)
          keepSessionDraft(draftName, kept, Object.values(kept))
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

    <PageCreateFooter />
  </Form>
}
