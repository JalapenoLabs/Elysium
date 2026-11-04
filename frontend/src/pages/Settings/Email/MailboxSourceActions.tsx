// Copyright © 2026 Jalapeno Labs

import type {
  MailCapabilities,
  MailServer,
  OAuthMailAccountKind,
} from '../../../api/routes/mailRoutes'

// Core
import { useTranslation } from 'react-i18next'

// User interface
import { Button, buttonVariants, Link, Tooltip } from '@heroui/react'
import { LuPlus } from 'react-icons/lu'

// Misc
import { getOAuthStartHref } from '../../../api/routes/mailRoutes'
import { UrlTree } from '../../../urls'
import {
  getMailboxBlockerKey,
  mailAccountKindIcons,
  mailAccountKindLabelKeys,
  OAUTH_MAIL_ACCOUNT_KINDS,
} from './mailPresentation'

type Props = {
  // Undefined while loading: every source shows, disabled, so the row does not jump.
  capabilities: MailCapabilities | undefined
  // Null while loading.
  server: MailServer | null
  hasDomains: boolean
}

// Where mailboxes come from: one button per OAuth provider, and a link to create a mailbox
// on the bundled mail server. A source this deployment cannot use yet is disabled and says why.
export function MailboxSourceActions(props: Props) {
  const { t } = useTranslation('email')
  const capabilities = props.capabilities

  function unavailableReason(kind: OAuthMailAccountKind) {
    const provider = t(mailAccountKindLabelKeys[kind])
    if (!capabilities) {
      return null
    }
    if (!capabilities.brokerConfigured) {
      return t('availability.brokerMissing')
    }
    if (capabilities.brokerError) {
      return t('availability.brokerUnreachable.title')
    }
    if (!capabilities.oauthKinds.includes(kind)) {
      return t('availability.providerMissing', { provider })
    }
    return null
  }

  // Unknown while the server loads, when the button waits disabled with nothing to explain.
  const createBlockerKey = props.server
    ? getMailboxBlockerKey(props.server, props.hasDomains)
    : null
  const canCreateMailbox = Boolean(props.server) && !createBlockerKey

  return <div className='flex flex-wrap items-center gap-2'>
    {OAUTH_MAIL_ACCOUNT_KINDS.map((kind) => {
      const Icon = mailAccountKindIcons[kind]
      const label = t('mailboxes.connect', { provider: t(mailAccountKindLabelKeys[kind]) })
      const reason = unavailableReason(kind)
      const isAvailable = Boolean(capabilities) && !reason

      if (isAvailable) {
        // A plain anchor: the API answers with a redirect to the broker, so this must be a
        // full page navigation rather than a client-side route change.
        return <a
          key={kind}
          href={getOAuthStartHref(kind)}
          className={buttonVariants({ size: 'sm', variant: 'outline' })}
        >
          <Icon className='size-4' aria-hidden />
          <span>{label}</span>
        </a>
      }

      return <Tooltip key={kind} delay={200} isDisabled={!reason}>
        <Tooltip.Trigger>
          <div>
            <Button size='sm' variant='outline' isDisabled>
              <Icon className='size-4' aria-hidden />
              <span>{label}</span>
            </Button>
          </div>
        </Tooltip.Trigger>
        <Tooltip.Content className='max-w-xs'>
          <span>{reason}</span>
        </Tooltip.Content>
      </Tooltip>
    })}

    {canCreateMailbox
      ? <Link href={UrlTree.settingsEmailMailboxNew} className={buttonVariants({ size: 'sm' })}>
        <LuPlus className='size-4' aria-hidden />
        <span>{t('mailboxes.create')}</span>
      </Link>
      : <Tooltip delay={200} isDisabled={!createBlockerKey}>
        <Tooltip.Trigger>
          <div>
            <Button size='sm' isDisabled>
              <LuPlus className='size-4' aria-hidden />
              <span>{t('mailboxes.create')}</span>
            </Button>
          </div>
        </Tooltip.Trigger>
        <Tooltip.Content className='max-w-xs'>
          <span>{createBlockerKey && t(createBlockerKey)}</span>
        </Tooltip.Content>
      </Tooltip>}
  </div>
}
