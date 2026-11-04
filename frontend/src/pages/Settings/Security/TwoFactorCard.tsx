// Copyright © 2026 Jalapeno Labs

import type { SettingsFlow } from '@ory/client-fetch'

// Core
import { useTranslation } from 'react-i18next'

// Redux
import { selectMe } from '../../../store/authSlice'
import { useAppSelector } from '../../../store/hooks'

// User interface
import { buttonVariants, Chip, Link } from '@heroui/react'
import { LuPencil, LuShieldPlus, LuTrash2 } from 'react-icons/lu'
import { SecurityCard } from './SecurityCard'

// Misc
import { UrlTree } from '../../../urls'
import { readAuthenticator, readLookupCodes } from './securityPresentation'

type Props = {
  flow: SettingsFlow
}

// Whether two-factor authentication is on and what it uses. Adding, changing, and removing it
// each have a page of their own (`~/.claude/docs/ux.md`, 2FA), so the buttons are links.
export function TwoFactorCard(props: Props) {
  const { t } = useTranslation([ 'auth', 'common' ])
  const me = useAppSelector(selectMe)
  const authenticator = readAuthenticator(props.flow)
  const lookup = readLookupCodes(props.flow)
  const isOn = authenticator.kind === 'enrolled'
  const hasLookupCodes = lookup.canReveal || lookup.canDisable
  const nickname = me?.authenticatorNickname ?? ''

  const authenticatorState = isOn
    ? nickname || t('security.twoFactor.on')
    : t('security.twoFactor.notSetUp')

  return <SecurityCard
    title={t('security.twoFactor.title')}
    status={<Chip size='sm' variant='soft' color={isOn
      ? 'success'
      : 'default'}>{
        isOn
          ? t('security.twoFactor.on')
          : t('security.twoFactor.off')
      }</Chip>}
    actions={isOn
      ? <>
        <Link href={UrlTree.settingsTwoFactorEdit} className={buttonVariants({ variant: 'outline' })}>
          <LuPencil className='size-4' aria-hidden />
          <span>{t('security.twoFactor.change')}</span>
        </Link>
        <Link href={UrlTree.settingsTwoFactorRemove} className={buttonVariants({ variant: 'ghost' })}>
          <LuTrash2 className='size-4' aria-hidden />
          <span>{t('security.twoFactor.remove')}</span>
        </Link>
      </>
      : <Link href={UrlTree.settingsTwoFactorNew} className={buttonVariants({ variant: 'outline' })}>
        <LuShieldPlus className='size-4' aria-hidden />
        <span>{t('security.twoFactor.add')}</span>
      </Link>}
  >
    <dl className='grid max-w-md grid-cols-[auto_1fr] gap-x-6 gap-y-1 text-sm'>
      <dt className='opacity-70'>{t('security.twoFactor.authenticatorApp')}</dt>
      <dd>{authenticatorState}</dd>
      <dt className='opacity-70'>{t('security.twoFactor.lookupCodes')}</dt>
      <dd>{
        hasLookupCodes
          ? t('security.twoFactor.on')
          : t('security.twoFactor.off')
      }</dd>
    </dl>
  </SecurityCard>
}
