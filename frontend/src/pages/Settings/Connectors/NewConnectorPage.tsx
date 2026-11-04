// Copyright © 2026 Jalapeno Labs

import type { ConnectorKind } from '../../../api/routes/connectorRoutes'

// Core
import { useTranslation } from 'react-i18next'
import { Navigate, useParams } from 'react-router'

// User interface
import { Spinner } from '@heroui/react'
import { SettingsPage } from '../SettingsPage'
import { ConnectorLogo } from './ConnectorLogo'
import { NewBrowserConnector } from './NewBrowserConnector'
import { NewTokenConnectorForm } from './NewTokenConnectorForm'

// Misc
import { CONNECTOR_KINDS } from '../../../api/routes/connectorRoutes'
import { UrlTree } from '../../../urls'
import { useConnectorKinds } from './useConnectorKinds'

// `/settings/connectors/new/:kind`: one kind's form, modeled on Onyx's. A kind that signs in
// with a token takes its account, what it reads, and its checks here, and is created once they
// have run; a kind read through a browser starts its browser here and is signed in to on its
// page. An unknown kind goes back to the grid.
export function NewConnectorPage() {
  const { t } = useTranslation('connectors')
  const { kind = '' } = useParams()
  const definitions = useConnectorKinds()

  const knownKind = CONNECTOR_KINDS.find((candidate) => candidate === kind)
  if (!knownKind) {
    console.debug('NewConnectorPage was opened for a kind it does not know', { kind })
    return <Navigate to={UrlTree.settingsConnectorNew} replace />
  }
  const connectorKind: ConnectorKind = knownKind
  const name = t(`kinds.${connectorKind}.name`)
  const parents = [
    { label: t('page.title'), href: UrlTree.settingsConnectors },
    { label: t('picker.title'), href: UrlTree.settingsConnectorNew },
  ]

  const definition = definitions.kindOf(connectorKind)
  const accountDefinition = definition
    ? definitions.accountOf(definition.account)
    : null

  // A personal kind's reading is its person's alone, but what it reads is in the knowledge base
  // every member searches (docs/knowledge.md, Who sees what), so it says so where it is added.
  const help = definition?.scope === 'personal'
    ? t('form.sharedKnowledge', { name })
    : undefined

  return <SettingsPage
    title={name}
    icon={<ConnectorLogo kind={connectorKind} className='size-8' />}
    parents={parents}
    help={help}
  >
    {(!definition || !accountDefinition) && <div className='grid place-items-center py-10'><Spinner /></div>}
    {definition?.account === 'browser' && <NewBrowserConnector definition={definition} />}
    {definition && accountDefinition && definition.account !== 'browser' && <NewTokenConnectorForm
      definition={definition}
      accountDefinition={accountDefinition}
    />}
  </SettingsPage>
}
