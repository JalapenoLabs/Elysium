// Copyright © 2026 Jalapeno Labs

import type { AccessToken } from '../../../proto/generated/security_pb'

// Core
import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'

// UI
import { Button, Chip } from '@heroui/react'
import { LuPlus, LuTrash2 } from 'react-icons/lu'
import { SettingsSection } from '../../../components/settings/SettingsSection'
import { OhLookAtTheTime } from '../../../components/OhLookAtTheTime'
import { TokenModal } from './TokenModal'

// Misc
import { useConfirm } from '../../../hooks/useConfirm'
import { useDisplayTimezone } from '../../../hooks/useDisplayTimezone'
import { deleteToken, listTokens } from '../../../api/routes/userRoutes'
import { tokenScopeLabelKey } from '../../../integrations/tokenScopes'

// The access-tokens credential section (#34): scoped tokens for HTTPS git,
// `docker login`, and SMTP AUTH. Each row shows its scopes as chips, its
// expiry, and last-used time, with mint and revoke.
export function TokensSection() {
  const { t } = useTranslation([ 'auth', 'common' ])
  const confirm = useConfirm()
  const displayTimezone = useDisplayTimezone()

  const [ tokens, setTokens ] = useState<AccessToken[]>([])
  const [ isLoaded, setIsLoaded ] = useState(false)
  const [ isModalOpen, setIsModalOpen ] = useState(false)
  const [ errorMessage, setErrorMessage ] = useState<string | null>(null)

  async function loadTokens() {
    try {
      const list = await listTokens()
      setTokens(list.tokens)
    }
    catch (error) {
      console.debug('access tokens unavailable', { error })
    }
    finally {
      setIsLoaded(true)
    }
  }

  useEffect(() => {
    loadTokens()
  }, [])

  function onRevoke(token: AccessToken) {
    confirm({
      title: t('auth:settings.tokens.removeTitle'),
      message: t('auth:settings.tokens.removeBody', { name: token.name }),
      confirmLabel: t('auth:settings.tokens.removeAction'),
      confirmColor: 'danger',
      onConfirm: async () => {
        setErrorMessage(null)
        try {
          await deleteToken(token.id)
          await loadTokens()
        }
        catch (error) {
          console.debug('token revoke failed', { error })
          setErrorMessage(t('auth:settings.tokens.updateFailed'))
        }
      },
    })
  }

  return <>
    <TokenModal
      isOpen={isModalOpen}
      onClose={() => setIsModalOpen(false)}
      onCreated={loadTokens}
    />

    <SettingsSection
      title={t('auth:settings.tokens.title')}
      description={t('auth:settings.tokens.body')}
      action={
        <Button
          size='sm'
          radius='sm'
          variant='bordered'
          startContent={<LuPlus className='h-3.5 w-3.5' />}
          onPress={() => setIsModalOpen(true)}
        >
          <span>{ t('auth:settings.tokens.add') }</span>
        </Button>
      }
    >
      { errorMessage
        ? <p className='mb-3 text-sm text-danger'>{ errorMessage }</p>
        : null }
      { tokens.length
        ? <div className='divide-y divide-divider'>
          { tokens.map((token) =>
            <div key={token.id} className='flex items-center justify-between gap-4 py-3 text-sm'>
              <div className='min-w-0'>
                <p className='truncate'>{ token.name }</p>
                <div className='mt-1 flex flex-wrap gap-1'>
                  { token.scopes.map((scope) =>
                    <Chip key={scope} size='sm' variant='flat' radius='sm'>
                      { t(`common:${tokenScopeLabelKey[scope]}`) }
                    </Chip>,
                  ) }
                </div>
              </div>
              <div className='flex shrink-0 items-center gap-3'>
                <span className='text-default-500'>
                  { token.expiresAt
                    ? <>
                      { t('auth:settings.tokens.expiresPrefix') }
                      { ' ' }
                      <OhLookAtTheTime timestamp={token.expiresAt} relative timezone={displayTimezone} />
                    </>
                    : t('auth:settings.tokens.neverExpires') }
                </span>
                <Button
                  isIconOnly
                  size='sm'
                  variant='light'
                  color='danger'
                  aria-label={t('auth:settings.tokens.removeAction')}
                  onPress={() => onRevoke(token)}
                >
                  <LuTrash2 className='h-4 w-4' />
                </Button>
              </div>
            </div>,
          ) }
        </div>
        : isLoaded
          ? <p className='text-sm text-default-500'>{ t('auth:settings.tokens.empty') }</p>
          : null }
    </SettingsSection>
  </>
}
