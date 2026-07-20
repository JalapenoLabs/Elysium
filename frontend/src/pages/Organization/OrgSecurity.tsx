// Copyright © 2026 Jalapeno Labs

import type { FormEvent } from 'react'
import type { OrgAuthPolicy } from '../../api/routes/orgRoutes'

// Core
import { useEffect, useState } from 'react'
import { useNavigate } from 'react-router'
import { useTranslation } from 'react-i18next'

// Redux
import { useAppSelector } from '../../store/hooks'

// UI
import { Card, CardBody, Select, SelectItem, Switch } from '@heroui/react'
import { MemberLinkingCard } from './MemberLinkingCard'
import { SubmitButton } from '../../components/form/SubmitButton'

// Utility
import { HTTPError } from 'ky'

// Misc
import { getOrgAuthPolicy, updateOrgAuthPolicy } from '../../api/routes/orgRoutes'
import { UrlTree } from '../../urls'

const EDITOR_ROLES = [ 'owner', 'admin' ]

// The session-lifetime choices, in seconds, within the server's 5min..90d bounds.
const lifetimeOptions = [
  { key: '3600', labelKey: 'orgSecurity.lifetimes.hour1' },
  { key: '43200', labelKey: 'orgSecurity.lifetimes.hour12' },
  { key: '86400', labelKey: 'orgSecurity.lifetimes.day1' },
  { key: '604800', labelKey: 'orgSecurity.lifetimes.week1' },
  { key: '2592000', labelKey: 'orgSecurity.lifetimes.day30' },
] as const

// The org's sign-in policy: which methods members may use, whether a second
// factor is mandatory, and how long sessions live. Admins and owners edit;
// everyone else sees it read-only. The server is the real authority (403 on an
// unauthorized write, 400 if a change would lock everyone out).
export function OrgSecurity() {
  const { t } = useTranslation([ 'auth', 'common' ])
  const navigate = useNavigate()

  const orgs = useAppSelector((state) => state.sessionContext.orgs)
  const activeOrgId = useAppSelector((state) => state.sessionContext.activeOrgId)
  const activeOrg = orgs.find((org) => org.id === activeOrgId)
  const canEdit = Boolean(activeOrg && EDITOR_ROLES.includes(activeOrg.role))

  const [ policy, setPolicy ] = useState<OrgAuthPolicy | null>(null)
  const [ isSaving, setIsSaving ] = useState(false)
  const [ isSaved, setIsSaved ] = useState(false)
  const [ errorMessage, setErrorMessage ] = useState<string | null>(null)

  useEffect(() => {
    if (!activeOrgId) {
      return undefined
    }
    let cancelled = false
    getOrgAuthPolicy(activeOrgId)
      .then((loaded) => {
        if (!cancelled) {
          setPolicy(loaded)
        }
      })
      .catch((error) => {
        console.debug('auth policy unavailable', { error })
        if (!cancelled) {
          setErrorMessage(t('auth:orgSecurity.loadFailed'))
        }
      })
    return () => {
      cancelled = true
    }
  }, [ activeOrgId, t ])

  function patch(fields: Partial<OrgAuthPolicy>) {
    setPolicy((current) => current && { ...current, ...fields })
    setIsSaved(false)
  }

  // At least one method must stay enabled; the server enforces this too, but
  // catching it here avoids a guaranteed-to-fail request.
  const noMethodEnabled = Boolean(policy)
    && !policy!.allow_passkeys
    && !policy!.allow_passwords
    && !policy!.allow_magic_links

  async function onSave(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    if (!activeOrgId || !policy) {
      return
    }
    if (noMethodEnabled) {
      setErrorMessage(t('auth:orgSecurity.noMethod'))
      return
    }

    setIsSaving(true)
    setIsSaved(false)
    setErrorMessage(null)
    try {
      const updated = await updateOrgAuthPolicy(activeOrgId, policy)
      setPolicy(updated)
      setIsSaved(true)
    }
    catch (error) {
      if (error instanceof HTTPError && error.response.status === 403) {
        navigate(UrlTree.accessDenied)
        return
      }
      console.debug('auth policy save failed', { error })
      setErrorMessage(t('auth:orgSecurity.saveFailed'))
    }
    finally {
      setIsSaving(false)
    }
  }

  return <section className='w-full'>
    <div className='relaxed'>
      <h2 className='text-2xl'>
        <strong>{ t('auth:orgSecurity.title') }</strong>
      </h2>
      <p className='opacity-80'>{ t('auth:orgSecurity.subtitle') }</p>
    </div>

    { policy
      ? <form onSubmit={onSave}>
        <Card className='p-1'>
          <CardBody>
            <div className='compact level items-start'>
              <div>
                <p>{ t('auth:orgSecurity.passkeys') }</p>
                <p className='text-sm opacity-60'>{ t('auth:orgSecurity.passkeysHint') }</p>
              </div>
              <Switch
                isSelected={policy.allow_passkeys}
                isDisabled={!canEdit}
                onValueChange={(value) => patch({ allow_passkeys: value })}
              />
            </div>
            <div className='compact level items-start'>
              <div>
                <p>{ t('auth:orgSecurity.passwords') }</p>
                <p className='text-sm opacity-60'>{ t('auth:orgSecurity.passwordsHint') }</p>
              </div>
              <Switch
                isSelected={policy.allow_passwords}
                isDisabled={!canEdit}
                onValueChange={(value) => patch({ allow_passwords: value })}
              />
            </div>
            <div className='compact level items-start'>
              <div>
                <p>{ t('auth:orgSecurity.magicLinks') }</p>
                <p className='text-sm opacity-60'>{ t('auth:orgSecurity.magicLinksHint') }</p>
              </div>
              <Switch
                isSelected={policy.allow_magic_links}
                isDisabled={!canEdit}
                onValueChange={(value) => patch({ allow_magic_links: value })}
              />
            </div>
            <div className='compact level items-start'>
              <div>
                <p>{ t('auth:orgSecurity.requireTotp') }</p>
                <p className='text-sm opacity-60'>{ t('auth:orgSecurity.requireTotpHint') }</p>
              </div>
              <Switch
                isSelected={policy.require_totp}
                isDisabled={!canEdit}
                onValueChange={(value) => patch({ require_totp: value })}
              />
            </div>
            <div className='compact'>
              <Select
                label={t('auth:orgSecurity.sessionLifetime')}
                isDisabled={!canEdit}
                selectedKeys={[ String(policy.session_lifetime_seconds) ]}
                onChange={(event) => {
                  const seconds = Number(event.target.value)
                  if (seconds) {
                    patch({ session_lifetime_seconds: seconds })
                  }
                }}
              >
                { lifetimeOptions.map((option) =>
                  <SelectItem key={option.key}>{ t(`auth:${option.labelKey}`) }</SelectItem>,
                ) }
              </Select>
            </div>

            { !canEdit
              ? <p className='mt-2 text-sm opacity-60'>{ t('auth:orgSecurity.readOnly') }</p>
              : null }
          </CardBody>
        </Card>

        <div className='level mt-6'>
          <div>
            { errorMessage
              ? <p className='text-sm text-danger'>{ errorMessage }</p>
              : null }
            { isSaved
              ? <p className='text-sm text-success'>{ t('auth:orgSecurity.saved') }</p>
              : null }
          </div>
          { canEdit
            ? <SubmitButton
              fullWidth={false}
              label={t('common:actions.save')}
              isValid={!noMethodEnabled}
              invalidReason={t('auth:orgSecurity.noMethod')}
              isSubmitting={isSaving}
            />
            : null }
        </div>
      </form>
      : errorMessage
        ? <p className='text-sm text-danger'>{ errorMessage }</p>
        : <p className='opacity-60'>{ t('auth:orgSecurity.loading') }</p> }

    { activeOrgId
      ? <MemberLinkingCard orgId={activeOrgId} canManage={canEdit} />
      : null }
  </section>
}
