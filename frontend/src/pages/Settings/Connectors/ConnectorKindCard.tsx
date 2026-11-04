// Copyright © 2026 Jalapeno Labs

import type { ConnectorKind } from '../../../api/routes/connectorRoutes'

// Core
import { useTranslation } from 'react-i18next'
import { Link } from 'react-router'

// User interface
import { Button, Card, Popover, Tooltip } from '@heroui/react'
import { LuCircleHelp } from 'react-icons/lu'
import { ConnectorLogo } from './ConnectorLogo'

// Misc
import { getNewConnectorUrl } from '../../../urls'

type Props = {
  kind: ConnectorKind
}

// One kind in the add-connector grid: its logo and name, opening its form, and a `?` that
// explains what it reads. The name's link stretches over the card, so the whole card opens the
// form; the `?` sits above it.
export function ConnectorKindCard(props: Props) {
  const { t } = useTranslation('connectors')
  const name = t(`kinds.${props.kind}.name`)

  return <Card className='relative p-4 transition-colors hover:bg-surface-secondary'>
    <div className='level gap-3'>
      <div className='flex min-w-0 items-center gap-3'>
        <ConnectorLogo kind={props.kind} className='size-6' />
        <Link
          to={getNewConnectorUrl(props.kind)}
          className='truncate font-semibold after:absolute after:inset-0 after:content-[""]'
        >
          {name}
        </Link>
      </div>
      <Popover>
        <Tooltip delay={400}>
          <Button
            isIconOnly
            size='sm'
            variant='ghost'
            className='relative z-10'
            aria-label={t('picker.about', { name })}
          >
            <LuCircleHelp className='size-4' aria-hidden />
          </Button>
          <Tooltip.Content>{t('picker.about', { name })}</Tooltip.Content>
        </Tooltip>
        <Popover.Content placement='bottom end' className='max-w-xs'>
          <Popover.Dialog>
            <Popover.Heading className='font-semibold'>{name}</Popover.Heading>
            <p className='mt-1 text-sm opacity-80'>{t(`kinds.${props.kind}.description`)}</p>
          </Popover.Dialog>
        </Popover.Content>
      </Popover>
    </div>
  </Card>
}
