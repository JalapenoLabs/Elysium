// Copyright © 2026 Jalapeno Labs

import type { ConnectedAccount, OauthProvider } from '../../../api/routes/oauthRoutes'

// Core
import { useEffect, useState } from 'react'
import { useSearchParams } from 'react-router'
import { useTranslation } from 'react-i18next'

// UI
import { Avatar, Button, Chip } from '@heroui/react'
import { SettingsSection } from '../../../components/settings/SettingsSection'

// Utility
import moment from 'moment-timezone'

// Misc
import { useConfirm } from '../../../hooks/useConfirm'
import { providerIcon } from '../../../integrations/oauthProviders'
import {
  connectProvider,
  disconnectAccount,
  listConnectedAccounts,
  listOauthProviders,
} from '../../../api/routes/oauthRoutes'

// External accounts the user has linked (#35, docs/integrations.md): connect a
// GitHub/Google/Microsoft account (several of a kind are allowed), see each
// with its external handle and scopes, reconnect a grant whose silent refresh
// has failed, and disconnect. Connecting is a full redirect; the OAuth callback
// returns here with `?connected=` or `?oauth_error=`, read on mount.
export function ConnectedAccountsSection() {
  const { t } = useTranslation([ 'auth', 'common' ])
  const confirm = useConfirm()
  const [ searchParams, setSearchParams ] = useSearchParams()

  const [ providers, setProviders ] = useState<OauthProvider[]>([])
  const [ accounts, setAccounts ] = useState<ConnectedAccount[]>([])
  const [ isLoaded, setIsLoaded ] = useState(false)
  const [ busyProvider, setBusyProvider ] = useState<string | null>(null)
  const [ notice, setNotice ] = useState<{ kind: 'success' | 'error', text: string } | null>(null)

  async function load() {
    try {
      const [ providerList, accountList ] = await Promise.all([
        listOauthProviders(),
        listConnectedAccounts(),
      ])
      setProviders(providerList)
      setAccounts(accountList)
    }
    catch (error) {
      console.debug('connected accounts unavailable', { error })
    }
    finally {
      setIsLoaded(true)
    }
  }

  useEffect(() => {
    load()
  }, [])

  // The OAuth callback redirects back here; surface the outcome once, then
  // strip the param so a refresh does not re-show it.
  useEffect(() => {
    const connected = searchParams.get('connected')
    const oauthError = searchParams.get('oauth_error')
    if (!connected && !oauthError) {
      return
    }

    if (connected) {
      setNotice({ kind: 'success', text: t('auth:settings.connections.connected', { provider: connected }) })
    }
    else {
      setNotice({ kind: 'error', text: t('auth:settings.connections.connectFailed') })
    }

    const nextParams = new URLSearchParams(searchParams)
    nextParams.delete('connected')
    nextParams.delete('oauth_error')
    setSearchParams(nextParams, { replace: true })
    load()
  }, [ searchParams ])

  async function onConnect(provider: string) {
    setBusyProvider(provider)
    setNotice(null)
    try {
      const response = await connectProvider(provider)
      // Full redirect; the backend owns state/PKCE and returns to this route.
      window.location.assign(response.authorize_url)
    }
    catch (error) {
      console.debug('oauth connect failed to start', { error })
      setNotice({ kind: 'error', text: t('auth:settings.connections.connectFailed') })
      setBusyProvider(null)
    }
  }

  function onDisconnect(account: ConnectedAccount) {
    const label = account.external_username || account.external_email || account.provider
    confirm({
      title: t('auth:settings.connections.disconnectTitle'),
      message: t('auth:settings.connections.disconnectBody', { name: label }),
      confirmLabel: t('auth:settings.connections.disconnectAction'),
      confirmColor: 'danger',
      onConfirm: async () => {
        setNotice(null)
        try {
          await disconnectAccount(account.id)
          await load()
        }
        catch (error) {
          console.debug('disconnect failed', { error })
          setNotice({ kind: 'error', text: t('auth:settings.connections.updateFailed') })
        }
      },
    })
  }

  // Only configured providers can be connected; an admin wires their creds.
  const connectableProviders = providers.filter((provider) => provider.configured)

  return <SettingsSection
    title={t('auth:settings.connections.title')}
    description={t('auth:settings.connections.body')}
  >
    { notice
      ? <p className={`mb-3 text-sm ${notice.kind === 'error' ? 'text-danger' : 'text-success'}`}>
        { notice.text }
      </p>
      : null }

    { accounts.length
      ? <div className='divide-y divide-divider'>
        { accounts.map((account) => {
          const ProviderIcon = providerIcon(account.provider)
          return <div key={account.id} className='flex items-center justify-between gap-4 py-3 text-sm'>
            <div className='flex min-w-0 items-center gap-3'>
              <Avatar
                size='sm'
                radius='sm'
                src={account.avatar_url || undefined}
                fallback={<ProviderIcon className='h-4 w-4' />}
                showFallback
              />
              <div className='min-w-0'>
                <p className='flex items-center gap-2 truncate'>
                  <span>{ account.external_username || account.external_email }</span>
                  { account.needs_reconsent
                    ? <Chip size='sm' variant='flat' radius='sm' color='warning'>
                      { t('auth:settings.connections.needsReconnect') }
                    </Chip>
                    : null }
                </p>
                <p className='truncate text-xs text-default-500'>
                  { account.connected_at
                    ? t('auth:settings.connections.connectedWhen', {
                      when: moment(account.connected_at).fromNow(),
                    })
                    : null }
                  { account.scopes.length
                    ? ` · ${account.scopes.join(', ')}`
                    : '' }
                </p>
              </div>
            </div>
            <div className='flex shrink-0 items-center gap-2'>
              { account.needs_reconsent
                ? <Button
                  size='sm'
                  radius='sm'
                  variant='flat'
                  color='primary'
                  isLoading={busyProvider === account.provider}
                  onPress={() => onConnect(account.provider)}
                >
                  <span>{ t('auth:settings.connections.reconnect') }</span>
                </Button>
                : null }
              <Button
                size='sm'
                radius='sm'
                variant='light'
                color='danger'
                onPress={() => onDisconnect(account)}
              >
                <span>{ t('auth:settings.connections.disconnect') }</span>
              </Button>
            </div>
          </div>
        }) }
      </div>
      : isLoaded
        ? <p className='text-sm text-default-500'>{ t('auth:settings.connections.empty') }</p>
        : null }

    { connectableProviders.length
      ? <div className='mt-4 flex flex-wrap gap-2'>
        { connectableProviders.map((provider) => {
          const ProviderIcon = providerIcon(provider.provider)
          return <Button
            key={provider.provider}
            size='sm'
            radius='sm'
            variant='bordered'
            startContent={<ProviderIcon className='h-4 w-4' />}
            isLoading={busyProvider === provider.provider}
            onPress={() => onConnect(provider.provider)}
          >
            <span>{ t('auth:settings.connections.connect', { provider: provider.display_name }) }</span>
          </Button>
        }) }
      </div>
      : null }
  </SettingsSection>
}
