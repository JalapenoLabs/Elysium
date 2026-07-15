// Copyright © 2026 Jalapeno Labs

import type { FormEvent } from 'react'

// Core
import { useEffect, useRef, useState } from 'react'
import { useNavigate } from 'react-router'
import { useTranslation } from 'react-i18next'

// Redux
import { dispatch } from '../../store'
import { useAppSelector } from '../../store/hooks'
import { orgPatched } from '../../store/sessionContextSlice'

// UI
import { Avatar, Card, CardBody, Input } from '@heroui/react'
import { SubmitButton } from '../../components/form/SubmitButton'

// Utility
import { HTTPError } from 'ky'

// Misc
import { updateOrg } from '../../api/routes/orgRoutes'
import { UrlTree } from '../../urls'

// Roles allowed to edit the org's identity; everyone else gets a read-only
// view. The server enforces this too (403), this just keeps honest UI.
const EDITOR_ROLES = [ 'owner', 'admin' ]

// The active organization's identity and branding: name, logo, accent color,
// and wordmark. Saving fans out `org.updated` to every member so their chrome
// updates live. This is the "General" section of the organization settings.
export function OrgGeneral() {
  const { t } = useTranslation([ 'auth', 'common' ])
  const navigate = useNavigate()

  const orgs = useAppSelector((state) => state.sessionContext.orgs)
  const activeOrgId = useAppSelector((state) => state.sessionContext.activeOrgId)
  const activeOrg = orgs.find((org) => org.id === activeOrgId)
  const canEdit = Boolean(activeOrg && EDITOR_ROLES.includes(activeOrg.role))

  const [ name, setName ] = useState('')
  const [ logoUrl, setLogoUrl ] = useState('')
  const [ accentColor, setAccentColor ] = useState('')
  const [ wordmark, setWordmark ] = useState('')
  const [ isSaving, setIsSaving ] = useState(false)
  const [ isSaved, setIsSaved ] = useState(false)
  const [ errorMessage, setErrorMessage ] = useState<string | null>(null)

  // Seed from the session context; edits already in progress win over reseeds.
  useEffect(() => {
    if (!activeOrg) {
      return
    }
    setName((current) => current || activeOrg.name)
    setLogoUrl((current) => current || activeOrg.logoUrl)
    setAccentColor((current) => current || activeOrg.accentColor)
    setWordmark((current) => current || activeOrg.wordmark)
  }, [ activeOrg ])

  const nameRef = useRef<HTMLInputElement>(null)
  useEffect(() => {
    nameRef.current?.focus()
  }, [])

  const canSubmit = canEdit && Boolean(name.trim())

  async function onSave(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    if (!activeOrg) {
      console.debug('org settings saved without an active org; ignoring')
      return
    }

    const trimmedAccent = accentColor.trim()
    if (trimmedAccent && !/^#[0-9a-fA-F]{6}$/.test(trimmedAccent)) {
      setErrorMessage(t('auth:settings.organization.accentInvalid'))
      return
    }

    setIsSaving(true)
    setIsSaved(false)
    setErrorMessage(null)
    try {
      const updated = await updateOrg(activeOrg.id, {
        name: name.trim(),
        logo_url: logoUrl.trim(),
        accent_color: trimmedAccent,
        wordmark: wordmark.trim(),
      })
      // The realtime push patches this too, but reflecting the response makes
      // the save feel instant even on a slow stream.
      dispatch(orgPatched({
        id: updated.id,
        name: updated.name,
        logoUrl: updated.logo_url,
        accentColor: updated.accent_color,
        wordmark: updated.wordmark,
      }))
      setIsSaved(true)
    }
    catch (error) {
      // Insufficient role is a real denial, not a form problem.
      if (error instanceof HTTPError && error.response.status === 403) {
        navigate(UrlTree.accessDenied)
        return
      }
      console.debug('org update failed', { error })
      setErrorMessage(t('auth:settings.organization.failed'))
    }
    finally {
      setIsSaving(false)
    }
  }

  return <section className='w-full'>
    <div className='relaxed'>
      <h2 className='text-2xl'>
        <strong>{ t('auth:settings.organization.title') }</strong>
      </h2>
      <p className='opacity-80'>{ t('auth:settings.organization.subtitle') }</p>
    </div>

    <form onSubmit={onSave}>
      <Card className='p-1'>
        <CardBody>
          <div className='compact level items-center'>
            <Avatar
              size='lg'
              radius='md'
              name={name || activeOrg?.name}
              src={logoUrl.trim() || undefined}
            />
            <div className='w-full'>
              <div className='compact'>
                <Input
                  ref={nameRef}
                  isDisabled={!canEdit}
                  label={t('auth:settings.organization.nameLabel')}
                  value={name}
                  onChange={(event) => setName(event.currentTarget.value)}
                />
              </div>
              <div className='compact'>
                <Input
                  isDisabled={!canEdit}
                  label={t('auth:settings.organization.logoLabel')}
                  placeholder='https://example.com/logo.png'
                  description={t('auth:settings.organization.logoHint')}
                  value={logoUrl}
                  onChange={(event) => setLogoUrl(event.currentTarget.value)}
                />
              </div>
              <div className='compact level items-center'>
                <Input
                  isDisabled={!canEdit}
                  className='w-full'
                  label={t('auth:settings.organization.accentLabel')}
                  placeholder='#F97316'
                  description={t('auth:settings.organization.accentHint')}
                  value={accentColor}
                  onChange={(event) => setAccentColor(event.currentTarget.value)}
                />
                {/* A native swatch picker feeding the same hex value. */}
                <input
                  type='color'
                  aria-label={t('auth:settings.organization.accentLabel')}
                  disabled={!canEdit}
                  className='h-10 w-14 shrink-0 cursor-pointer rounded-lg border-none bg-transparent'
                  value={/^#[0-9a-fA-F]{6}$/.test(accentColor.trim()) ? accentColor.trim() : '#6366f1'}
                  onChange={(event) => setAccentColor(event.currentTarget.value)}
                />
              </div>
              <div className='compact'>
                <Input
                  isDisabled={!canEdit}
                  label={t('auth:settings.organization.wordmarkLabel')}
                  description={t('auth:settings.organization.wordmarkHint')}
                  value={wordmark}
                  onChange={(event) => setWordmark(event.currentTarget.value)}
                />
              </div>
            </div>
          </div>

          { !canEdit
            ? <p className='mt-2 text-sm opacity-60'>
              { t('auth:settings.organization.readOnly') }
            </p>
            : null }
        </CardBody>
      </Card>

      <div className='level mt-6'>
        <div>
          { errorMessage
            ? <p className='text-sm text-danger'>{ errorMessage }</p>
            : null }
          { isSaved
            ? <p className='text-sm text-success'>{ t('auth:settings.organization.saved') }</p>
            : null }
        </div>
        <SubmitButton
          fullWidth={false}
          label={t('common:actions.save')}
          isValid={canSubmit}
          invalidReason={canEdit
            ? t('auth:settings.organization.required')
            : t('auth:settings.organization.readOnly')}
          isSubmitting={isSaving}
        />
      </div>
    </form>
  </section>
}
