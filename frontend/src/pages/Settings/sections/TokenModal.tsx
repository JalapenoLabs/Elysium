// Copyright © 2026 Jalapeno Labs

import type { Integration } from '../../../proto/generated/integration_pb'

// Core
import { useEffect, useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'

// UI
import {
  Button,
  Checkbox,
  Input,
  Modal,
  ModalBody,
  ModalContent,
  ModalFooter,
  ModalHeader,
  Select,
  SelectItem,
} from '@heroui/react'
import { OneTimeSecretCard } from '../../../components/enrollment/OneTimeSecretCard'

// Misc
import { listIntegrations } from '../../../api/routes/integrationRoutes'
import { mintToken } from '../../../api/routes/userRoutes'
import { selectableTokenScopes, tokenScopeLabelKey } from '../../../integrations/tokenScopes'
import { TokenScope } from '../../../proto/generated/security_pb'

const SECONDS_PER_DAY = 86_400

// Expiry presets (in seconds; 0 = no expiry). Fixed choices keep the mint form
// simple; a custom TTL can come later if asked for.
const ttlPresets = [
  { key: '0', labelKey: 'auth:settings.tokens.ttlNever', seconds: 0 },
  { key: '30d', labelKey: 'auth:settings.tokens.ttl30', seconds: 30 * SECONDS_PER_DAY },
  { key: '90d', labelKey: 'auth:settings.tokens.ttl90', seconds: 90 * SECONDS_PER_DAY },
  { key: '365d', labelKey: 'auth:settings.tokens.ttl365', seconds: 365 * SECONDS_PER_DAY },
] as const

type Props = {
  isOpen: boolean
  onClose: () => void
  onCreated: () => void
}

// Mints a scoped access token: a name, one or more scopes, an optional bind to
// a single integration (default: all reachable instances of the scoped kinds),
// and an optional expiry. The `ely_pat_` secret is revealed exactly once; the
// modal transitions collect -> reveal in place.
export function TokenModal(props: Props) {
  const { t } = useTranslation([ 'auth', 'common' ])

  const [ name, setName ] = useState('')
  const [ scopes, setScopes ] = useState<TokenScope[]>([])
  const [ integrationId, setIntegrationId ] = useState('')
  const [ ttlKey, setTtlKey ] = useState<string>('0')
  const [ integrations, setIntegrations ] = useState<Integration[]>([])
  const [ isBusy, setIsBusy ] = useState(false)
  const [ errorMessage, setErrorMessage ] = useState<string | null>(null)
  const [ revealedSecret, setRevealedSecret ] = useState<string | null>(null)

  useEffect(() => {
    if (!props.isOpen) {
      return
    }
    setName('')
    setScopes([])
    setIntegrationId('')
    setTtlKey('0')
    setErrorMessage(null)
    setRevealedSecret(null)
    // The bind picker lists every integration the user can reach.
    listIntegrations()
      .then((response) => setIntegrations(response.integrations))
      .catch((error) => {
        console.debug('integrations unavailable for the token bind picker', { error })
      })
  }, [ props.isOpen ])

  const ttlSeconds = useMemo(
    () => ttlPresets.find((preset) => preset.key === ttlKey)?.seconds ?? 0,
    [ ttlKey ],
  )

  const canSubmit = Boolean(name.trim()) && Boolean(scopes.length) && !isBusy

  function toggleScope(scope: TokenScope, isSelected: boolean) {
    setScopes((previous) => {
      if (isSelected) {
        return [ ...previous, scope ]
      }
      return previous.filter((existing) => existing !== scope)
    })
  }

  async function onSubmit() {
    setIsBusy(true)
    setErrorMessage(null)
    try {
      const minted = await mintToken({
        name: name.trim(),
        scopes,
        integrationId,
        ttlSeconds,
      })
      props.onCreated()
      setRevealedSecret(minted.secret)
    }
    catch (error) {
      console.debug('token mint failed', { error })
      setErrorMessage(t('auth:settings.tokens.saveFailed'))
    }
    finally {
      setIsBusy(false)
    }
  }

  return <Modal isOpen={props.isOpen} onClose={props.onClose} size='lg' scrollBehavior='inside'>
    <ModalContent>
      { revealedSecret
        ? <ModalBody className='py-6'>
          <OneTimeSecretCard
            title={t('auth:settings.tokens.revealTitle')}
            subtitle={t('auth:settings.tokens.revealSubtitle')}
            secret={revealedSecret}
            copyLabel={t('common:actions.copy')}
            actionLabel={t('auth:settings.tokens.revealDone')}
            onDone={props.onClose}
          />
        </ModalBody>
        : <>
          <ModalHeader>{ t('auth:settings.tokens.addTitle') }</ModalHeader>
          <ModalBody>
            <div className='compact'>
              <Input
                autoFocus
                label={t('auth:settings.tokens.nameLabel')}
                value={name}
                isDisabled={isBusy}
                onValueChange={setName}
              />
            </div>
            <div className='compact'>
              <p className='compact text-sm font-medium opacity-70'>{
                t('auth:settings.tokens.scopesLabel')
              }</p>
              <div className='grid grid-cols-2 gap-2'>
                { selectableTokenScopes.map((scope) =>
                  <Checkbox
                    key={scope}
                    size='sm'
                    isSelected={scopes.includes(scope)}
                    isDisabled={isBusy}
                    onValueChange={(isSelected) => toggleScope(scope, isSelected)}
                  >
                    <span className='text-sm'>{ t(`common:${tokenScopeLabelKey[scope]}`) }</span>
                  </Checkbox>,
                ) }
              </div>
            </div>
            <div className='compact level items-start'>
              <Select
                className='w-full'
                label={t('auth:settings.tokens.bindLabel')}
                isDisabled={isBusy}
                selectedKeys={integrationId ? [ integrationId ] : [ 'all' ]}
                onChange={(event) => {
                  setIntegrationId(event.target.value === 'all' ? '' : event.target.value)
                }}
              >
                {/* Fragment groups the fixed "all" option with the dynamic
                    list; react-stately flattens it into the collection. */}
                <>
                  <SelectItem key='all'>{ t('auth:settings.tokens.bindAll') }</SelectItem>
                  { integrations.map((integration) =>
                    <SelectItem key={integration.id?.value ?? ''}>{ integration.name }</SelectItem>,
                  ) }
                </>
              </Select>
              <Select
                className='w-full'
                label={t('auth:settings.tokens.expiryLabel')}
                isDisabled={isBusy}
                selectedKeys={[ ttlKey ]}
                onChange={(event) => {
                  if (event.target.value) {
                    setTtlKey(event.target.value)
                  }
                }}
              >
                { ttlPresets.map((preset) =>
                  <SelectItem key={preset.key}>{ t(preset.labelKey) }</SelectItem>,
                ) }
              </Select>
            </div>
            { errorMessage
              ? <p className='text-sm text-danger'>{ errorMessage }</p>
              : null }
          </ModalBody>
          <ModalFooter>
            <Button variant='light' isDisabled={isBusy} onPress={props.onClose}>
              <span>{ t('common:actions.cancel') }</span>
            </Button>
            <Button color='primary' isDisabled={!canSubmit} isLoading={isBusy} onPress={onSubmit}>
              <span>{ t('auth:settings.tokens.addAction') }</span>
            </Button>
          </ModalFooter>
        </> }
    </ModalContent>
  </Modal>
}
