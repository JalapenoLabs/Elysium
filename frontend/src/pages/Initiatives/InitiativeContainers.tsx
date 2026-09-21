// Copyright © 2026 Jalapeno Labs

import type { Initiative, InitiativeLink } from '../../api/routes/initiativeRoutes'

// Core
import { useTranslation } from 'react-i18next'

// Redux
import { useAppDispatch, useAppSelector } from '../../store/hooks'
import { initiativeLinkDeleted, selectInitiativeLinks } from '../../store/initiativeLinksSlice'

// User interface
import { Button, Chip, Link, Spinner, Tooltip, toast, useOverlayState } from '@heroui/react'
import { LuExternalLink, LuPlus, LuUnlink } from 'react-icons/lu'
import { AddContainerModal } from './AddContainerModal'

// Misc
import { removeInitiativeLink } from '../../api/routes/initiativeRoutes'
import { useConfirm } from '../../hooks/useConfirm'
import { useInitiativeLinksLoader } from '../../hooks/useServerData'
import { providerLabelKeys } from '../ActionItems/linkPresentation'
import { containerKindLabelKeys } from './containerPresentation'

type Props = {
  initiative: Initiative
}

// The epics, filters, milestones, and labels an initiative follows. Their open issues join it
// as items, and leave it when the container is unlinked, so nothing is tracked twice.
export function InitiativeContainers(props: Props) {
  const { t, i18n } = useTranslation([ 'initiatives', 'actionItems', 'common' ])
  const dispatch = useAppDispatch()
  const confirm = useConfirm()
  const status = useInitiativeLinksLoader(props.initiative.id)
  const links = useAppSelector((state) => selectInitiativeLinks(state, props.initiative.id))
  const addState = useOverlayState()
  const isReadOnly = Boolean(props.initiative.deletedAt)
  const dateFormatter = new Intl.DateTimeFormat(i18n.language, { dateStyle: 'medium', timeStyle: 'short' })

  function remove(link: InitiativeLink) {
    confirm({
      title: t('containers.removeTitle', { name: link.title }),
      message: t('containers.removeBody'),
      tone: 'danger',
      confirmText: t('containers.remove'),
      onConfirm: async () => {
        try {
          await removeInitiativeLink(link.initiativeId, link.id)
        }
        catch (error) {
          console.debug('InitiativeContainers failed to unlink a container', { error, linkId: link.id })
          toast.danger(t('common:errors.unexpected'))
          throw error
        }
        dispatch(initiativeLinkDeleted(link.id))
        toast.success(t('containers.unlinked', { name: link.title }))
      },
    })
  }

  function describeSync(link: InitiativeLink) {
    if (link.syncError) {
      return <p className='mt-1 text-xs text-warning'>{t('containers.syncError', { error: link.syncError })}</p>
    }
    if (!link.syncedAt) {
      return <p className='mt-1 text-xs opacity-60'>{t('containers.waiting')}</p>
    }
    return <p className='mt-1 text-xs opacity-60'>{
      t('containers.synced', { date: dateFormatter.format(new Date(link.syncedAt)) })
    }</p>
  }

  return <section className='relaxed'>
    <div className='level compact'>
      <h2 className='text-lg font-semibold'>{t('containers.heading')}</h2>
      {!isReadOnly && <Button size='sm' variant='secondary' onPress={addState.open}>
        <LuPlus className='size-4' aria-hidden />
        <span>{t('containers.add')}</span>
      </Button>}
    </div>

    {status === 'loading' && <div className='grid place-items-center py-6'>
      <Spinner size='sm' />
    </div>}
    {status === 'failed' && <p className='text-sm text-danger'>{t('containers.loadError')}</p>}
    {status === 'loaded' && !links.length && <p className='text-sm opacity-70'>{t('containers.empty')}</p>}

    <ul className='flex flex-col gap-3'>{
      links.map((link) => <li key={link.id} className='rounded-xl bg-surface p-3'>
        <div className='level gap-3'>
          <div className='flex min-w-0 flex-wrap items-center gap-2'>
            <Chip size='sm' variant='soft'>{t(providerLabelKeys[link.provider], { ns: 'actionItems' })}</Chip>
            <Chip size='sm' variant='soft'>{t(containerKindLabelKeys[link.kind])}</Chip>
            <Link href={link.url} target='_blank' rel='noreferrer' className='text-link min-w-0 truncate text-sm'>
              {link.title}
              <LuExternalLink className='ml-1 inline size-3' aria-hidden />
            </Link>
          </div>
          {!isReadOnly && <Tooltip delay={300}>
            <Button
              isIconOnly
              size='sm'
              variant='ghost'
              aria-label={t('containers.remove')}
              onPress={() => remove(link)}
            >
              <LuUnlink className='size-4' aria-hidden />
            </Button>
            <Tooltip.Content>
              <span>{t('containers.remove')}</span>
            </Tooltip.Content>
          </Tooltip>}
        </div>
        {describeSync(link)}
        {link.truncated && <p className='mt-1 text-xs text-warning'>{t('containers.truncated')}</p>}
      </li>)
    }</ul>

    <AddContainerModal initiativeId={props.initiative.id} state={addState} />
  </section>
}
