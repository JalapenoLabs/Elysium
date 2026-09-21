// Copyright © 2026 Jalapeno Labs

import type { Key } from '@heroui/react'
import type { ActionItem, ActionItemLink, LinkRemoteRead, LinkTarget } from '../../api/routes/actionItemRoutes'

// Core
import { useTranslation } from 'react-i18next'

// Redux
import { useAppDispatch, useAppSelector } from '../../store/hooks'
import {
  actionItemLinkDeleted,
  actionItemLinksLoaded,
  actionItemLinkUpserted,
  selectActionItemLinks,
} from '../../store/actionItemLinksSlice'

// User interface
import { Button, Chip, Dropdown, Label, Link, Spinner, Tooltip, toast, useOverlayState } from '@heroui/react'
import { LuEllipsis, LuExternalLink, LuPlus } from 'react-icons/lu'
import { LinkTargetModal } from './LinkTargetModal'
import { PendingWrites } from './PendingWrites'

// Utility
import { HTTPError } from 'ky'

// Misc
import { getApiErrorMessage } from '../../api/errors'
import {
  addActionItemLink,
  makePrimaryActionItemLink,
  removeActionItemLink,
} from '../../api/routes/actionItemRoutes'
import { useConfirm } from '../../hooks/useConfirm'
import { useActionItemLinkRemotes, useActionItemLinksLoader } from '../../hooks/useServerData'
import {
  linkKindLabelKeys,
  linkStateChipColors,
  linkStateLabelKeys,
  providerLabelKeys,
} from './linkPresentation'

type Props = {
  item: ActionItem
}

// The Jira issues, GitHub issues, and pull requests an item stands for. What each looks like
// now is read live from its provider; what the item last recorded shows until that answers,
// and stays when it cannot.
export function ActionItemLinks(props: Props) {
  const { t } = useTranslation([ 'actionItems', 'common' ])
  const dispatch = useAppDispatch()
  const status = useActionItemLinksLoader(props.item.id)
  const links = useAppSelector((state) => selectActionItemLinks(state, props.item.id))
  const { remotes, status: remoteStatus } = useActionItemLinkRemotes(props.item.id)
  const addState = useOverlayState()
  const isReadOnly = Boolean(props.item.deletedAt)

  const remoteByLinkId = new Map<string, LinkRemoteRead>()
  for (const read of remotes ?? []) {
    remoteByLinkId.set(read.linkId, read)
  }

  async function link(target: LinkTarget) {
    try {
      const response = await addActionItemLink(props.item.id, target)
      dispatch(actionItemLinkUpserted(response.link))
      toast.success(t('toasts.linked', { key: response.link.key }))
    }
    catch (error) {
      if (error instanceof HTTPError && error.response.status === 409) {
        toast.danger(t('toasts.alreadyLinked'))
        throw error
      }
      console.debug('ActionItemLinks failed to link', { error, itemId: props.item.id })
      toast.danger(getApiErrorMessage(error) ?? t('common:errors.unexpected'))
      throw error
    }
  }

  return <section className='relaxed'>
    <div className='level compact'>
      <h2 className='text-lg font-semibold'>{t('links.heading')}</h2>
      {!isReadOnly && <Button size='sm' variant='secondary' onPress={addState.open}>
        <LuPlus className='size-4' aria-hidden />
        <span>{t('links.add')}</span>
      </Button>}
    </div>

    {status === 'loading' && <div className='grid place-items-center py-6'>
      <Spinner size='sm' />
    </div>}
    {status === 'failed' && <p className='text-sm text-danger'>{t('links.loadError')}</p>}
    {status === 'loaded' && !links.length && <p className='text-sm opacity-70'>{t('links.empty')}</p>}

    <ul className='flex flex-col gap-3'>{
      links.map((link) => <LinkRow
        key={link.id}
        link={link}
        read={remoteByLinkId.get(link.id)}
        isReading={remoteStatus === 'loading'}
        isReadOnly={isReadOnly}
      />)
    }</ul>

    <LinkTargetModal
      state={addState}
      title={t('links.picker.linkTitle')}
      description={t('links.picker.linkDescription')}
      submitLabel={t('links.picker.linkSubmit')}
      onSubmit={link}
    />
  </section>
}

type LinkRowProps = {
  link: ActionItemLink
  // Undefined until the live read answers.
  read: LinkRemoteRead | undefined
  isReading: boolean
  isReadOnly: boolean
}

function LinkRow(props: LinkRowProps) {
  const { t, i18n } = useTranslation([ 'actionItems', 'common' ])
  const dispatch = useAppDispatch()
  const confirm = useConfirm()
  const link = props.link
  const remote = props.read?.remote ?? null
  const provider = t(providerLabelKeys[link.provider])
  // The live state wins; the recorded one shows until it answers.
  const state = remote?.state ?? link.state

  async function makePrimary() {
    try {
      const response = await makePrimaryActionItemLink(link.actionItemId, link.id)
      dispatch(actionItemLinksLoaded({ actionItemId: link.actionItemId, links: response.links }))
      toast.success(t('toasts.primaryChanged', { key: link.key }))
    }
    catch (error) {
      console.debug('ActionItemLinks failed to change the primary link', { error, linkId: link.id })
      toast.danger(t('common:errors.unexpected'))
    }
  }

  function remove() {
    confirm({
      title: t('links.removeTitle', { key: link.key }),
      message: t('links.removeBody', { provider }),
      tone: 'danger',
      confirmText: t('links.remove'),
      onConfirm: async () => {
        try {
          await removeActionItemLink(link.actionItemId, link.id)
        }
        catch (error) {
          console.debug('ActionItemLinks failed to unlink', { error, linkId: link.id })
          toast.danger(t('common:errors.unexpected'))
          throw error
        }
        dispatch(actionItemLinkDeleted(link.id))
        toast.success(t('toasts.unlinked', { key: link.key }))
      },
    })
  }

  const actions: Record<string, () => void> = {
    primary: () => void makePrimary(),
    remove,
  }

  // The provider's own fields, joined on one quiet line.
  const details: string[] = []
  if (remote) {
    details.push(remote.status)
    details.push(remote.assignee
      ? t('links.assignee', { name: remote.assignee })
      : t('links.unassigned'))
    if (remote.priority) {
      details.push(t('links.providerPriority', { provider, priority: remote.priority }))
    }
    if (remote.dueDate) {
      // A day, not an instant, so it is shown as the provider wrote it.
      const dueFormatter = new Intl.DateTimeFormat(i18n.language, { dateStyle: 'medium', timeZone: 'UTC' })
      details.push(t('links.providerDue', {
        provider,
        date: dueFormatter.format(new Date(`${remote.dueDate}T00:00:00Z`)),
      }))
    }
  }

  return <li className='rounded-xl bg-surface p-3'>
    <div className='level gap-3'>
      <div className='flex min-w-0 flex-wrap items-center gap-2'>
        <Chip size='sm' variant='soft'>{provider}</Chip>
        <Chip size='sm' variant='soft'>{t(linkKindLabelKeys[link.kind])}</Chip>
        <Link
          href={link.url}
          target='_blank'
          rel='noreferrer'
          className='text-link font-mono text-sm'
        >
          {link.key}
          <LuExternalLink className='ml-1 inline size-3' aria-hidden />
          <span className='sr-only'>{t('links.openExternal', { provider })}</span>
        </Link>
        <span className='min-w-0 truncate text-sm'>{remote?.title ?? link.title}</span>
      </div>
      <div className='flex shrink-0 items-center gap-2'>
        <Chip size='sm' variant='soft' color={linkStateChipColors[state]}>{t(linkStateLabelKeys[state])}</Chip>
        {link.isPrimary && <Tooltip delay={300}>
          <Tooltip.Trigger>
            <Chip size='sm' variant='soft' color='accent'>{t('links.primary')}</Chip>
          </Tooltip.Trigger>
          <Tooltip.Content>
            <span>{t('links.primaryHint')}</span>
          </Tooltip.Content>
        </Tooltip>}
        {!props.isReadOnly && <Dropdown>
          <Tooltip delay={300}>
            <Button isIconOnly size='sm' variant='ghost' aria-label={t('common:actions.moreActions')}>
              <LuEllipsis className='size-4' aria-hidden />
            </Button>
            <Tooltip.Content>
              <span>{t('common:actions.moreActions')}</span>
            </Tooltip.Content>
          </Tooltip>
          <Dropdown.Popover placement='bottom end'>
            <Dropdown.Menu onAction={(key: Key) => actions[String(key)]?.()}>
              {!link.isPrimary && <Dropdown.Item id='primary' textValue={t('links.makePrimary')}>
                <Label>{t('links.makePrimary')}</Label>
              </Dropdown.Item>}
              <Dropdown.Item id='remove' textValue={t('links.remove')} variant='danger'>
                <Label>{t('links.remove')}</Label>
              </Dropdown.Item>
            </Dropdown.Menu>
          </Dropdown.Popover>
        </Dropdown>}
      </div>
    </div>

    {props.isReading && <p className='mt-1 text-xs opacity-60'>{t('links.reading', { provider })}</p>}
    {props.read?.error && <p className='mt-1 text-xs text-warning'>{
      t('links.readError', { provider, error: props.read.error })
    }</p>}
    {details.length > 0 && <p className='mt-1 text-xs opacity-70'>{details.join(' · ')}</p>}

    <PendingWrites link={link} isReadOnly={props.isReadOnly} />
  </li>
}
