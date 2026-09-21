// Copyright © 2026 Jalapeno Labs

// Core
import { useTranslation } from 'react-i18next'
import { useParams } from 'react-router'

// Redux
import { useAppDispatch, useAppSelector } from '../../store/hooks'
import { actionItemUpserted, selectActionItemById } from '../../store/actionItemsSlice'

// User interface
import { Alert, Breadcrumbs, Link, Spinner, toast } from '@heroui/react'
import { InlineEditableText } from '../../components/InlineEditableText'
import { ActionItemActionBar } from './ActionItemActionBar'
import { ActionItemBadges } from './ActionItemBadges'
import { ActionItemComments } from './ActionItemComments'
import { ActionItemDetailsPanel } from './ActionItemDetailsPanel'
import { ActionItemHistory } from './ActionItemHistory'
import { ActionItemLinks } from './ActionItemLinks'
import { ActionItemSessions } from './ActionItemSessions'
import { RestoreActionItemButton } from './RestoreActionItemButton'

// Misc
import { updateActionItem } from '../../api/routes/actionItemRoutes'
import { useActionItemLoader } from '../../hooks/useServerData'
import { useNow } from '../../hooks/useNow'
import { UrlTree } from '../../urls'
import { NOTES_MAX_CHARACTERS, TITLE_MAX_CHARACTERS } from './actionItemFormSchema'

// `/action-items/:itemId`: one item, edited in place. The title and notes are edited where
// they are shown, its fields sit in a panel beside them, and its links to Jira and GitHub,
// its conversation, the coding sessions started from it, and its history follow. A deleted
// item is shown read-only, with Restore.
export function ActionItemPage() {
  const { t } = useTranslation([ 'actionItems', 'common' ])
  const dispatch = useAppDispatch()
  const { itemId = '' } = useParams()
  const status = useActionItemLoader(itemId)
  const item = useAppSelector((state) => selectActionItemById(state, itemId))
  const now = useNow()

  if (!item) {
    return <div className='container'>{
      status === 'loading'
        ? <div className='grid place-items-center py-16'>
          <Spinner />
        </div>
        : <div className='py-16 text-center'>
          <p className='compact text-sm opacity-70'>{t('item.notFound')}</p>
          <Link href={UrlTree.actionItemsAll} className='text-link'>{t('item.backToList')}</Link>
        </div>
    }</div>
  }

  const isDeleted = Boolean(item.deletedAt)

  // Throws on failure, so the field stays open on what was typed.
  async function saveText(changes: { title: string } | { notes: string }) {
    if (!item) {
      console.debug('ActionItemPage saved a field with no item loaded')
      return
    }
    try {
      const response = await updateActionItem(item.id, changes)
      dispatch(actionItemUpserted(response.item))
    }
    catch (error) {
      console.debug('ActionItemPage failed to save the item', { error, itemId: item.id })
      toast.danger(t('common:errors.unexpected'))
      throw error
    }
  }

  return <div className='container'>
    <Breadcrumbs className='compact'>
      <Breadcrumbs.Item href={UrlTree.actionItems}>{t('title')}</Breadcrumbs.Item>
      <Breadcrumbs.Item href={UrlTree.actionItemsAll}>{t('nav.all')}</Breadcrumbs.Item>
      <Breadcrumbs.Item>{item.title}</Breadcrumbs.Item>
    </Breadcrumbs>

    {isDeleted && <Alert status='warning' className='relaxed'>
      <Alert.Indicator />
      <Alert.Content>
        <Alert.Title>{t('item.deletedTitle')}</Alert.Title>
        <Alert.Description>{t('item.deletedBody')}</Alert.Description>
      </Alert.Content>
      <RestoreActionItemButton item={item} />
    </Alert>}

    <div className='relaxed'>
      {isDeleted
        ? <h1 className='compact text-3xl font-bold'>{item.title}</h1>
        : <div className='compact'>
          <InlineEditableText
            value={item.title}
            label={t('item.renameLabel')}
            isRequired
            maxLength={TITLE_MAX_CHARACTERS}
            className='text-3xl font-bold'
            onSave={(title) => saveText({ title })}
          />
        </div>}
      <div className='compact'>
        <ActionItemBadges item={item} now={now} showState />
      </div>
      {!isDeleted && <ActionItemActionBar item={item} now={now} />}
    </div>

    <div className='grid grid-cols-1 items-start gap-8 lg:grid-cols-[minmax(0,1fr)_minmax(0,20rem)]'>
      <div>
        <section className='relaxed'>
          <h2 className='compact text-lg font-semibold'>{t('item.notesHeading')}</h2>
          {isDeleted
            ? <p className='text-sm whitespace-pre-line opacity-80'>{item.notes}</p>
            : <InlineEditableText
              value={item.notes}
              label={t('item.notesLabel')}
              placeholder={t('item.addNotes')}
              isMultiline
              maxLength={NOTES_MAX_CHARACTERS}
              className='text-sm whitespace-pre-line opacity-80'
              onSave={(notes) => saveText({ notes })}
            />}
        </section>
        <ActionItemLinks item={item} />
        <section className='relaxed'>
          <h2 className='compact text-lg font-semibold'>{t('comments.heading')}</h2>
          <ActionItemComments item={item} />
        </section>
        <section className='relaxed'>
          <h2 className='compact text-lg font-semibold'>{t('sessions.heading')}</h2>
          <ActionItemSessions itemId={item.id} />
        </section>
        <section>
          <h2 className='compact text-lg font-semibold'>{t('history.heading')}</h2>
          <ActionItemHistory itemId={item.id} />
        </section>
      </div>
      <aside className='lg:sticky lg:top-4'>
        <ActionItemDetailsPanel item={item} />
      </aside>
    </div>
  </div>
}
