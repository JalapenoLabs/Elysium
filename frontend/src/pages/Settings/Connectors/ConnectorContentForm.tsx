// Copyright © 2026 Jalapeno Labs

import type { Connector, ConnectorKindDefinition } from '../../../api/routes/connectorRoutes'

// Core
import { useState } from 'react'
import { useTranslation } from 'react-i18next'

// Redux
import { connectorUpserted } from '../../../store/connectorsSlice'
import { useAppDispatch } from '../../../store/hooks'

// User interface
import { Card, Form } from '@heroui/react'
import { ContentFields } from './ContentFields'

// Misc
import { updateConnector } from '../../../api/routes/connectorRoutes'
import { usePageSave } from '../../../layout/page/pageSave'
import { connectorErrorMessage } from './connectorPresentation'
import { toast } from '../../../toast/toast'

type Props = {
  connector: Connector
  definition: ConnectorKindDefinition
}

// What a stored connector reads, changed in place and saved with the page's Save. Options are
// listed through the account the connector owns.
export function ConnectorContentForm(props: Props) {
  const { t } = useTranslation([ 'connectors', 'common' ])
  const dispatch = useAppDispatch()
  const [ content, setContent ] = useState<Record<string, unknown> | null>(null)
  const connector = props.connector
  // A storage location is kept on the connector itself, beside its content, rather than in it.
  const storageField = props.definition.content.find((field) => field.field.type === 'storage_location')
  const stored = storageField
    ? { [storageField.id]: connector.storageLocationId, ...connector.content }
    : connector.content
  const shown = content ?? stored

  async function save() {
    if (!content) {
      console.debug('ConnectorContentForm was saved with nothing changed', { connectorId: connector.id })
      return false
    }
    try {
      const response = await updateConnector(connector.id, { content })
      dispatch(connectorUpserted(response.connector))
      setContent(null)
      toast.success(t('actions.saved'))
      return true
    }
    catch (error) {
      console.debug('ConnectorContentForm failed to save', { error, connectorId: connector.id })
      toast.danger(connectorErrorMessage(error, t))
      return false
    }
  }

  const onSubmit = usePageSave({
    save,
    isDirty: content !== null,
  })

  return <Form className='relaxed' onSubmit={onSubmit}>
    <Card className='w-full p-6'>
      <h2 className='compact font-semibold'>{t('detail.content')}</h2>
      <ContentFields
        kind={connector.kind}
        fields={props.definition.content}
        account={connector.account
          ? { id: connector.account.id }
          : null}
        value={shown}
        onChange={setContent}
      />
    </Card>
  </Form>
}
