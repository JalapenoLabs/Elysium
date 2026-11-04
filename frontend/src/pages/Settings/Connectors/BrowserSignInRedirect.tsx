// Copyright © 2026 Jalapeno Labs

// Core
import { Navigate, useParams } from 'react-router'

// Redux
import { selectAllConnectors } from '../../../store/connectorsSlice'
import { useAppSelector } from '../../../store/hooks'

// User interface
import { Spinner } from '@heroui/react'

// Misc
import { useConnectorsLoader } from '../../../hooks/useServerData'
import { UrlTree, getConnectorUrl } from '../../../urls'
import { connectorOwningBrowser } from './connectorPresentation'

// `/settings/browsers/:browserSessionId`, from before browsers lived in their connectors: a
// browser's address opens the connector that owns it, and anything else the Connectors list.
export function BrowserSignInRedirect() {
  const { browserSessionId = '' } = useParams()
  const status = useConnectorsLoader()
  const connectors = useAppSelector(selectAllConnectors)

  if (status === 'loading') {
    return <div className='grid place-items-center py-10'><Spinner /></div>
  }
  const owner = connectorOwningBrowser(connectors, browserSessionId)
  if (!owner) {
    console.debug('BrowserSignInRedirect found no connector for a browser', { browserSessionId })
    return <Navigate to={UrlTree.settingsConnectors} replace />
  }
  return <Navigate to={getConnectorUrl(owner.id)} replace />
}
