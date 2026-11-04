// Copyright © 2026 Jalapeno Labs

import type { LookupCode } from './securityPresentation'

// Core
import { useTranslation } from 'react-i18next'

// User interface
import { Alert, Button, buttonVariants, Link, Spinner, Tooltip } from '@heroui/react'
import { LuArrowLeft } from 'react-icons/lu'
import { BackupCodesList } from './BackupCodesList'

// Misc
import { LEAVE_WITHOUT_ASKING } from '../../../layout/page/pageSave'

type Props = {
  // The new codes, once Kratos made them; null while they are made.
  codes: LookupCode[] | null
  appName: string
  // The person has backup codes already, which these replace once 2FA is on.
  replacesCodes: boolean
  isDownloaded: boolean
  onDownloaded: () => void
  // Making the codes failed; offer to try again.
  hasFailed: boolean
  onRetry: () => void
  backHref: string
  onNext: () => void
}

// The second step of adding two-factor authentication: the backup codes, which go on only
// once they are downloaded (`~/.claude/docs/ux.md`, 2FA). Nothing is saved yet: the codes
// wait in their Kratos flow until the app is verified.
export function TwoFactorCodesStep(props: Props) {
  const { t } = useTranslation([ 'auth', 'common' ])

  const next = <Button isDisabled={!props.isDownloaded} onPress={props.onNext}>
    <span>{t('common:actions.next')}</span>
  </Button>

  return <div>
    {props.replacesCodes && <Alert status='warning' className='relaxed'>
      <Alert.Indicator />
      <Alert.Content>
        <Alert.Description>{t('security.backupCodes.replaceWarning')}</Alert.Description>
      </Alert.Content>
    </Alert>}

    {props.codes && <div className='relaxed'>
      <BackupCodesList
        codes={props.codes}
        appName={props.appName}
        onDownloaded={props.onDownloaded}
        isDownloadPrimary
      />
    </div>}

    {!props.codes && !props.hasFailed && <div className='relaxed grid place-items-center py-10'>
      <Spinner />
    </div>}

    {!props.codes && props.hasFailed && <div className='relaxed py-6'>
      <p className='compact text-sm text-danger'>{t('security.backupCodes.generateFailed')}</p>
      <Button size='sm' variant='outline' onPress={props.onRetry}>
        <span>{t('common:actions.retry')}</span>
      </Button>
    </div>}

    <div className='page-create-footer'>
      <Link
        href={props.backHref}
        routerOptions={{ state: LEAVE_WITHOUT_ASKING }}
        className={buttonVariants({ variant: 'outline' })}
      >
        <LuArrowLeft className='size-4' aria-hidden />
        <span>{t('common:actions.back')}</span>
      </Link>
      {props.isDownloaded
        ? next
        // A disabled button fires no hover events, so the tooltip's trigger wraps it.
        : <Tooltip delay={200}>
          <Tooltip.Trigger>
            <div>{next}</div>
          </Tooltip.Trigger>
          <Tooltip.Content className='max-w-xs'>
            <span>{t('security.backupCodes.downloadFirst')}</span>
          </Tooltip.Content>
        </Tooltip>}
    </div>
  </div>
}
