// Copyright © 2026 Jalapeno Labs

import type { Connector, ConnectorKind } from '../../../api/routes/connectorRoutes'

// Core
import { useTranslation } from 'react-i18next'
import { Link } from 'react-router'

// User interface
import { Card, Chip } from '@heroui/react'
import { ConnectorKindCard } from '../Connectors/ConnectorKindCard'
import { ConnectorLogo } from '../Connectors/ConnectorLogo'

// Misc
import { getConnectorUrl } from '../../../urls'
import { accountNameOf, statusColors } from '../Connectors/connectorPresentation'

type Props = {
  // Every kind that provides meetings.
  kinds: ConnectorKind[]
  // The person's connectors of those kinds.
  connectors: Connector[]
}

// Where the person's meetings come from. Each source is a connector, added, checked, and
// removed under Connectors, so each links to its connector's page. With none, it offers every
// kind that provides meetings, each opening its own form.
export function MeetingSources(props: Props) {
  const { t } = useTranslation([ 'meetings', 'connectors' ])

  if (!props.connectors.length) {
    return <section>
      <p className='compact opacity-70'>{t('sources.none')}</p>
      <div className='grid grid-cols-1 gap-3 sm:grid-cols-2 lg:grid-cols-3 xl:grid-cols-4'>{
        props.kinds.map((kind) => <ConnectorKindCard key={kind} kind={kind} />)
      }</div>
    </section>
  }

  return <section>
    <ul className='flex flex-wrap gap-3'>{
      props.connectors.map((connector) => {
        const accountName = accountNameOf(connector)
        return <li key={connector.id}>
          <Card className='relative p-3 transition-colors hover:bg-surface-secondary'>
            <div className='flex items-center gap-3'>
              <ConnectorLogo kind={connector.kind} />
              <div className='min-w-0'>
                <Link
                  to={getConnectorUrl(connector.id)}
                  className='font-semibold after:absolute after:inset-0 after:content-[""]'
                >
                  {t(`connectors:kinds.${connector.kind}.name`)}
                </Link>
                {accountName && <p className='truncate text-xs opacity-60'>{accountName}</p>}
              </div>
              <Chip size='sm' variant='soft' color={statusColors[connector.status]}>{
                t(`connectors:status.${connector.status}`)
              }</Chip>
            </div>
          </Card>
        </li>
      })
    }</ul>
  </section>
}
