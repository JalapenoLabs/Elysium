// Copyright © 2026 Jalapeno Labs

import type { IntegrationPolicy } from '../../proto/generated/integration_pb'

// Core
import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'

// UI
import { Card, CardBody, Switch } from '@heroui/react'

// Misc
import {
  getIntegrationPolicies,
  updateIntegrationPolicy,
} from '../../api/routes/integrationRoutes'
import { integrationKindMeta } from '../../integrations/model'

type Props = {
  orgId: string
  canManage: boolean
}

// The per-kind member-linking toggles (docs/integrations.md): whether members
// may connect personal accounts of each kind (defaults off). Each flip
// PATCHes one kind and re-renders from the returned full set, so the card
// always reflects what the server enforces.
export function MemberLinkingCard(props: Props) {
  const { t } = useTranslation([ 'auth', 'common' ])

  const [ policies, setPolicies ] = useState<IntegrationPolicy[]>([])
  const [ isBusy, setIsBusy ] = useState(false)
  const [ errorMessage, setErrorMessage ] = useState<string | null>(null)

  useEffect(() => {
    getIntegrationPolicies(props.orgId)
      .then((response) => setPolicies(response.policies))
      .catch((error) => {
        console.debug('integration policies unavailable', { error })
        setErrorMessage(t('auth:settings.integrations.linking.loadFailed'))
      })
  }, [ props.orgId ])

  async function onToggle(policy: IntegrationPolicy, membersMayLink: boolean) {
    setIsBusy(true)
    setErrorMessage(null)
    try {
      const response = await updateIntegrationPolicy(props.orgId, policy.kind, membersMayLink)
      setPolicies(response.policies)
    }
    catch (error) {
      console.debug('integration policy update failed', { error })
      setErrorMessage(t('auth:settings.integrations.linking.updateFailed'))
    }
    finally {
      setIsBusy(false)
    }
  }

  return <Card className='relaxed'>
    <CardBody>
      <div className='compact'>
        <p className='font-medium'>{ t('auth:settings.integrations.linking.title') }</p>
        <p className='text-sm opacity-70'>{ t('auth:settings.integrations.linking.subtitle') }</p>
      </div>
      { policies.map((policy) =>
        <div key={policy.kind} className='compact level items-center'>
          <span className='text-sm'>{
            t(`common:${integrationKindMeta[policy.kind].labelKey}`)
          }</span>
          <Switch
            size='sm'
            isSelected={policy.membersMayLink}
            isDisabled={isBusy || !props.canManage}
            onValueChange={(membersMayLink) => onToggle(policy, membersMayLink)}
          />
        </div>,
      ) }
      { errorMessage
        ? <p className='text-sm text-danger'>{ errorMessage }</p>
        : null }
    </CardBody>
  </Card>
}
