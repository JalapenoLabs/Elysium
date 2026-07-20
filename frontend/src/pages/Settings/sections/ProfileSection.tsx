// Copyright © 2026 Jalapeno Labs

import type { FormEvent } from 'react'

// Core
import { useEffect, useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'

// UI
import {
  Autocomplete,
  AutocompleteItem,
  Avatar,
  Button,
  Input,
  Select,
  SelectItem,
  Textarea,
} from '@heroui/react'
import { LuPencil } from 'react-icons/lu'
import { SettingsSection, DetailRow } from '../../../components/settings/SettingsSection'

// Misc
import { useCurrentUser } from '../../../auth/useCurrentUser'
import { updateMyProfile } from '../../../api/routes/userRoutes'
import { buildTimezoneOptions, resolveDisplayTimezone } from '../../../proto/helpers/timezone'
import { Language } from '../../../proto/generated/user_pb'

// The saved timezone is an IANA name, and empty means auto-detect. The picker
// needs a real key for that empty choice, so it rides this sentinel and maps
// back to '' on save.
const AUTO_DETECT_KEY = 'auto'

// The languages offered, in display order. Auto-detect (UNSPECIFIED) leads; add
// each new enum member here as its translation ships. `satisfies Record<Language,...>`
// keeps the label map exhaustive so a new language can't be forgotten.
const LANGUAGE_ORDER: Language[] = [ Language.UNSPECIFIED, Language.EN_US ]
const languageLabelKeyByCode = {
  [Language.UNSPECIFIED]: 'auth:settings.profile.language.autoDetect',
  [Language.EN_US]: 'auth:settings.profile.language.enUs',
} as const satisfies Record<Language, string>

// The "User" section. It keeps the same label/value rows in the same order in
// both modes (Stripe's pattern): Edit turns each value into an inline input, the
// labels on the left never move, and nothing reorders. Email is display-only
// (the profile update doesn't change it).
export function ProfileSection() {
  const { t } = useTranslation([ 'auth', 'common' ])
  const user = useCurrentUser()

  const [ isEditing, setIsEditing ] = useState(false)
  const [ givenName, setGivenName ] = useState('')
  const [ familyName, setFamilyName ] = useState('')
  const [ displayName, setDisplayName ] = useState('')
  const [ bio, setBio ] = useState('')
  const [ pronouns, setPronouns ] = useState('')
  const [ webLink, setWebLink ] = useState('')
  const [ contactPhone, setContactPhone ] = useState('')
  const [ backupEmail, setBackupEmail ] = useState('')
  const [ avatarUrl, setAvatarUrl ] = useState('')
  const [ language, setLanguage ] = useState<Language>(Language.UNSPECIFIED)
  const [ timezone, setTimezone ] = useState('')
  const [ isSubmitting, setIsSubmitting ] = useState(false)

  // ~600 zones, each needing an offset lookup, so build the list once per mount
  // rather than on every keystroke in the picker.
  const timezoneOptions = useMemo(() => buildTimezoneOptions(), [])
  const [ errorMessage, setErrorMessage ] = useState<string | null>(null)

  function seedForm() {
    setGivenName(user?.givenName ?? '')
    setFamilyName(user?.familyName ?? '')
    setDisplayName(user?.displayName ?? '')
    setBio(user?.bio ?? '')
    setPronouns(user?.pronouns ?? '')
    setWebLink(user?.webLink ?? '')
    setContactPhone(user?.contactPhone ?? '')
    setBackupEmail(user?.backupEmail ?? '')
    setAvatarUrl(user?.avatarUrl ?? '')
    setLanguage(user?.language ?? Language.UNSPECIFIED)
    setTimezone(user?.timezone ?? '')
  }

  useEffect(() => {
    if (!isEditing) {
      seedForm()
    }
  }, [ user ])

  function onEdit() {
    seedForm()
    setErrorMessage(null)
    setIsEditing(true)
  }

  function onCancel() {
    seedForm()
    setErrorMessage(null)
    setIsEditing(false)
  }

  async function onSave(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    if (!givenName.trim()) {
      return
    }
    setIsSubmitting(true)
    setErrorMessage(null)
    try {
      await updateMyProfile({
        givenName: givenName.trim(),
        familyName: familyName.trim(),
        displayName: displayName.trim(),
        bio: bio.trim(),
        pronouns: pronouns.trim(),
        webLink: webLink.trim(),
        contactPhone: contactPhone.trim(),
        backupEmail: backupEmail.trim(),
        language,
        timezone,
        avatarUrl: avatarUrl.trim(),
      })
      setIsEditing(false)
    }
    catch (error) {
      console.debug('profile save failed', { error })
      setErrorMessage(t('auth:settings.profile.failed'))
    }
    finally {
      setIsSubmitting(false)
    }
  }

  const fullName = [ user?.givenName, user?.familyName ].filter(Boolean).join(' ')
  const emptyValue = <span className='text-default-400'>{ t('auth:settings.profile.empty') }</span>

  const action = isEditing
    ? <Button size='sm' radius='sm' variant='light' onPress={onCancel}>
      <span>{ t('common:actions.cancel') }</span>
    </Button>
    : <Button
      size='sm'
      radius='sm'
      variant='bordered'
      startContent={<LuPencil className='h-3.5 w-3.5' />}
      onPress={onEdit}
    >
      <span>{ t('common:actions.edit') }</span>
    </Button>

  return <SettingsSection title={t('auth:settings.profile.sectionTitle')} action={action}>
    <form onSubmit={onSave}>
      <DetailRow label={t('auth:settings.profile.photoLabel')}>
        <div className='flex items-center gap-3'>
          <Avatar
            size='md'
            radius='sm'
            name={(isEditing ? displayName : user?.displayName) || fullName}
            src={(isEditing ? avatarUrl.trim() : user?.avatarUrl) || undefined}
          />
          { isEditing
            ? <Input
              size='sm'
              radius='sm'
              className='max-w-sm'
              aria-label={t('auth:settings.profile.avatarLabel')}
              placeholder='https://example.com/me.png'
              value={avatarUrl}
              onChange={(event) => setAvatarUrl(event.currentTarget.value)}
            />
            : null }
        </div>
      </DetailRow>

      <DetailRow label={t('auth:settings.profile.nameLabel')}>
        { isEditing
          ? <div className='flex max-w-sm gap-2'>
            <Input
              autoFocus
              size='sm'
              radius='sm'
              aria-label={t('auth:settings.profile.givenNameLabel')}
              placeholder={t('auth:settings.profile.givenNameLabel')}
              value={givenName}
              onChange={(event) => setGivenName(event.currentTarget.value)}
            />
            <Input
              size='sm'
              radius='sm'
              aria-label={t('auth:settings.profile.familyNameLabel')}
              placeholder={t('auth:settings.profile.familyNameLabel')}
              value={familyName}
              onChange={(event) => setFamilyName(event.currentTarget.value)}
            />
          </div>
          : (fullName || emptyValue) }
      </DetailRow>

      <DetailRow label={t('auth:settings.profile.displayNameLabel')}>
        { isEditing
          ? <Input
            size='sm'
            radius='sm'
            className='max-w-sm'
            aria-label={t('auth:settings.profile.displayNameLabel')}
            description={t('auth:settings.profile.displayNameHint')}
            value={displayName}
            onChange={(event) => setDisplayName(event.currentTarget.value)}
          />
          : (user?.displayName || emptyValue) }
      </DetailRow>

      <DetailRow label={t('auth:settings.profile.pronounsLabel')}>
        { isEditing
          ? <Input
            size='sm'
            radius='sm'
            className='max-w-sm'
            aria-label={t('auth:settings.profile.pronounsLabel')}
            placeholder={t('auth:settings.profile.pronounsPlaceholder')}
            value={pronouns}
            onChange={(event) => setPronouns(event.currentTarget.value)}
          />
          : (user?.pronouns || emptyValue) }
      </DetailRow>

      <DetailRow label={t('auth:settings.profile.emailLabel')}>
        { user?.email || emptyValue }
      </DetailRow>

      <DetailRow label={t('auth:settings.profile.backupEmailLabel')}>
        { isEditing
          ? <Input
            size='sm'
            type='email'
            radius='sm'
            className='max-w-sm'
            aria-label={t('auth:settings.profile.backupEmailLabel')}
            placeholder={t('auth:settings.profile.backupEmailPlaceholder')}
            value={backupEmail}
            onChange={(event) => setBackupEmail(event.currentTarget.value)}
          />
          : (user?.backupEmail || emptyValue) }
      </DetailRow>

      <DetailRow label={t('auth:settings.profile.contactPhoneLabel')}>
        { isEditing
          ? <Input
            size='sm'
            type='tel'
            radius='sm'
            className='max-w-sm'
            aria-label={t('auth:settings.profile.contactPhoneLabel')}
            placeholder={t('auth:settings.profile.contactPhonePlaceholder')}
            value={contactPhone}
            onChange={(event) => setContactPhone(event.currentTarget.value)}
          />
          : (user?.contactPhone || emptyValue) }
      </DetailRow>

      <DetailRow label={t('auth:settings.profile.webLinkLabel')}>
        { isEditing
          ? <Input
            size='sm'
            type='url'
            radius='sm'
            className='max-w-sm'
            aria-label={t('auth:settings.profile.webLinkLabel')}
            placeholder={t('auth:settings.profile.webLinkPlaceholder')}
            value={webLink}
            onChange={(event) => setWebLink(event.currentTarget.value)}
          />
          : (user?.webLink
            ? <a
              href={user.webLink}
              target='_blank'
              rel='noreferrer noopener'
              className='text-primary hover:underline'
            >
              { user.webLink }
            </a>
            : emptyValue) }
      </DetailRow>

      <DetailRow label={t('auth:settings.profile.bioLabel')}>
        { isEditing
          ? <Textarea
            size='sm'
            radius='sm'
            className='max-w-sm'
            aria-label={t('auth:settings.profile.bioLabel')}
            value={bio}
            onChange={(event) => setBio(event.currentTarget.value)}
            minRows={3}
          />
          : (user?.bio || emptyValue) }
      </DetailRow>

      <DetailRow label={t('auth:settings.profile.language.label')}>
        { isEditing
          ? <Select
            size='sm'
            radius='sm'
            className='max-w-sm'
            aria-label={t('auth:settings.profile.language.label')}
            disallowEmptySelection
            selectedKeys={[ String(language) ]}
            onChange={(event) => setLanguage(Number(event.target.value) as Language)}
          >
            { LANGUAGE_ORDER.map((code) =>
              <SelectItem key={String(code)}>{ t(languageLabelKeyByCode[code]) }</SelectItem>,
            ) }
          </Select>
          : t(languageLabelKeyByCode[user?.language ?? Language.UNSPECIFIED]) }
      </DetailRow>

      <DetailRow label={t('auth:settings.profile.timezone.label')}>
        { isEditing
          ? <Autocomplete
            size='sm'
            radius='sm'
            className='max-w-sm'
            aria-label={t('auth:settings.profile.timezone.label')}
            selectedKey={timezone || AUTO_DETECT_KEY}
            onSelectionChange={(key) => {
              // A cleared field keeps the current choice rather than wiping it.
              if (key === null) {
                return
              }
              setTimezone(key === AUTO_DETECT_KEY ? '' : String(key))
            }}
          >
            <AutocompleteItem key={AUTO_DETECT_KEY}>
              { t('auth:settings.profile.timezone.autoDetect') }
            </AutocompleteItem>
            <>
              { timezoneOptions.map((option) =>
                <AutocompleteItem key={option.zone}>{ option.label }</AutocompleteItem>,
              ) }
            </>
          </Autocomplete>
          : (user?.timezone
            || t('auth:settings.profile.timezone.autoDetectWith', {
              zone: resolveDisplayTimezone(),
            })) }
      </DetailRow>

      { isEditing
        ? <div className='mt-5'>
          { errorMessage
            ? <p className='mb-3 text-sm text-danger'>{ errorMessage }</p>
            : null }
          <Button
            type='submit'
            size='sm'
            radius='sm'
            color='primary'
            isLoading={isSubmitting}
            isDisabled={isSubmitting || !givenName.trim()}
          >
            <span>{ t('common:actions.save') }</span>
          </Button>
        </div>
        : null }
    </form>
  </SettingsSection>
}
