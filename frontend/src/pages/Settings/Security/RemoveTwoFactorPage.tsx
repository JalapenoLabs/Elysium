// Copyright © 2026 Jalapeno Labs

// Core
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { Navigate, useNavigate } from 'react-router'

// Redux
import { selectMe } from '../../../store/authSlice'
import { useAppSelector } from '../../../store/hooks'

// User interface
import { Button, buttonVariants, Label, Link, Spinner, Switch } from '@heroui/react'
import { LuArrowLeft } from 'react-icons/lu'
import { CountdownButton } from '../../../components/CountdownButton'
import { SettingsPage } from '../SettingsPage'

// Misc
import { useKratosSettingsFlow } from '../../../hooks/useKratosSettingsFlow'
import { useRecentSignIn } from '../../../hooks/useRecentSignIn'
import { LEAVE_WITHOUT_ASKING } from '../../../layout/page/pageSave'
import { UrlTree } from '../../../urls'
import { readAuthenticator, readLookupCodes } from './securityPresentation'
import { storeAuthenticatorNickname } from './twoFactorNickname'
import { toast } from '../../../toast/toast'

// How long Remove 2FA stays disabled, so what it does is read first (`~/.claude/docs/ux.md`,
// Delete confirmations).
const REMOVE_COUNTDOWN_SECONDS = 5

// `/settings/security/two-factor/remove`: turning two-factor authentication off. The app is
// removed, and with it, unless the person keeps them, the backup codes, which would otherwise
// still count as a second factor on their own.
export function RemoveTwoFactorPage() {
  const { t } = useTranslation([ 'auth', 'common' ])
  const navigate = useNavigate()
  const me = useAppSelector(selectMe)
  const isRecent = useRecentSignIn(UrlTree.settingsTwoFactorRemove)
  const settings = useKratosSettingsFlow(UrlTree.settingsTwoFactorRemove)
  const [ removesCodes, setRemovesCodes ] = useState(true)
  const [ isRemoving, setIsRemoving ] = useState(false)
  const flow = settings.flow
  const lookup = flow
    ? readLookupCodes(flow)
    : null
  const hasCodes = Boolean(lookup?.canReveal || lookup?.canDisable)
  const appName = me?.authenticatorNickname || t('security.twoFactor.unnamedApp')

  // Once the app is gone the flow shows none, so the page leaves only for someone who came
  // without one, not while it removes the app.
  if (flow && !isRemoving && readAuthenticator(flow).kind !== 'enrolled') {
    return <Navigate to={UrlTree.settingsSecurity} replace />
  }

  async function remove() {
    setIsRemoving(true)
    const isRemoved = await settings.submit({ method: 'totp', totp_unlink: true }, null)
    if (!isRemoved) {
      setIsRemoving(false)
      return
    }

    // The app is gone either way; the codes and the name are what is left to tidy.
    if (removesCodes && hasCodes) {
      const areCodesOff = await settings.submit({ method: 'lookup_secret', lookup_secret_disable: true }, null)
      if (!areCodesOff) {
        toast.warning(t('security.twoFactor.codesNotRemoved'))
      }
    }
    // A name only shows beside an app, so a failure here is harmless and the next app set up
    // replaces it.
    await storeAuthenticatorNickname('').catch((error: unknown) => {
      console.debug('RemoveTwoFactorPage could not forget the nickname', { error })
    })
    toast.success(t('security.twoFactor.removed'))
    void navigate(UrlTree.settingsSecurity, { state: LEAVE_WITHOUT_ASKING })
  }

  return <SettingsPage
    title={t('security.twoFactor.removeTitle')}
    parents={[{ label: t('security.title'), href: UrlTree.settingsSecurity }]}
  >
    {settings.loadFailed && <div className='py-10 text-center'>
      <p className='compact text-sm text-danger'>{t('security.loadError')}</p>
      <Button size='sm' variant='outline' onPress={settings.retry}>
        <span>{t('common:actions.retry')}</span>
      </Button>
    </div>}

    {(!flow || !isRecent) && !settings.loadFailed && <div className='grid place-items-center py-16'>
      <Spinner />
    </div>}

    {flow && isRecent && <section className='rounded-xl border border-danger p-6'>
      <p className='compact text-sm'>{t('security.twoFactor.removeMessage', { name: appName })}</p>
      <p className='relaxed text-sm font-semibold'>{t('common:confirm.irreversible')}</p>

      {hasCodes && <Switch
        id='remove-two-factor-codes'
        className='relaxed'
        isSelected={removesCodes}
        onChange={setRemovesCodes}
      >
        <Switch.Content>
          <Switch.Control>
            <Switch.Thumb />
          </Switch.Control>
          <Label>{t('security.twoFactor.removeCodesToo')}</Label>
        </Switch.Content>
      </Switch>}

      <div className='page-create-footer'>
        <Link href={UrlTree.settingsSecurity} className={buttonVariants({ variant: 'outline' })}>
          <LuArrowLeft className='size-4' aria-hidden />
          <span>{t('common:actions.back')}</span>
        </Link>
        <CountdownButton
          seconds={REMOVE_COUNTDOWN_SECONDS}
          label={t('security.twoFactor.remove')}
          variant='danger'
          isPending={isRemoving}
          onPress={() => void remove()}
        />
      </div>
    </section>}
  </SettingsPage>
}
