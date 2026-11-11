// Copyright © 2026 Jalapeno Labs

import type { ActionItem } from '../../api/routes/actionItemRoutes'

// Core
import { useWatch } from 'react-hook-form'
import { useTranslation } from 'react-i18next'

// User interface
import { Alert, Button, useOverlayState } from '@heroui/react'
import { InlineEditableText } from '../../components/InlineEditableText'
import { Page } from '../../layout/page/Page'
import { ActionItemReply } from '../../replies/ActionItemReply'
import { ActionItemBadges } from './ActionItemBadges'
import { ActionItemComments } from './ActionItemComments'
import { ActionItemDetailsPanel } from './ActionItemDetailsPanel'
import { ActionItemFollowUp } from './ActionItemFollowUp'
import { ActionItemHistory } from './ActionItemHistory'
import { ActionItemLinks } from './ActionItemLinks'
import { ActionItemManagerAssignment } from './ActionItemManagerAssignment'
import { ActionItemSessions } from './ActionItemSessions'
import { MeetingReviewLink } from './MeetingReviewLink'
import { MeetingThreadLink } from './MeetingThreadLink'
import { RestoreActionItemButton } from './RestoreActionItemButton'
import { SameMeetingPanel } from './SameMeetingPanel'
import { SnoozeDateModal } from './SnoozeDateModal'

// Misc
import { useNow } from '../../hooks/useNow'
import { UrlTree } from '../../urls'
import { NOTES_MAX_CHARACTERS, TITLE_MAX_CHARACTERS } from './actionItemFormSchema'
import { useActionItemActions } from './useActionItemActions'
import { useActionItemEditor } from './useActionItemEditor'
import { useActionItemPageActions } from './useActionItemPageActions'

type Props = {
  item: ActionItem
}

// One item, edited in place and kept by the page's Save. The title is the page's title,
// renamed where it is shown; the state change its state leads with is the page's primary
// action, and everything else it can do is its Actions menu. Its notes and fields sit side by
// side, and its links to Jira and GitHub, its assignment to its Project Manager with where its
// plan stands, the meeting it asks to review, the reply box of the
// message that raised it, the follow-up it is when it keeps a promise, its conversation, the
// coding sessions started from it, and its history follow. Those act at once, as actions do;
// only the fields wait for Save. A deleted item is read-only, with Restore.
export function ActionItemView(props: Props) {
  const { t } = useTranslation([ 'actionItems', 'common' ])
  const item = props.item
  const editor = useActionItemEditor(item)
  const [ title, notes ] = useWatch({ control: editor.form.control, name: [ 'title', 'notes' ]})
  const now = useNow()
  const snoozeDateDialog = useOverlayState()
  const itemActions = useActionItemActions()
  const pageActions = useActionItemPageActions({ item, now, onPickSnoozeDate: snoozeDateDialog.open })
  const isDeleted = Boolean(item.deletedAt)
  const lead = pageActions.lead

  return <Page
    title={isDeleted
      ? item.title
      : title}
    rename={isDeleted
      ? undefined
      : {
        label: t('item.renameLabel'),
        maxLength: TITLE_MAX_CHARACTERS,
        onChange: (value) => editor.form.setValue('title', value, { shouldDirty: true }),
      }}
    save={isDeleted
      ? undefined
      : { save: editor.save, isDirty: editor.form.formState.isDirty }}
    parents={[
      { label: t('title'), href: UrlTree.actionItems },
      { label: t('nav.all'), href: UrlTree.actionItemsAll },
    ]}
    primaryAction={!isDeleted && lead && <Button isPending={lead.isPending} onPress={lead.onAction}>
      <lead.icon className='size-4' aria-hidden />
      <span>{lead.label}</span>
    </Button>}
    actions={isDeleted
      ? undefined
      : pageActions.actions}
  >
    {isDeleted && <Alert status='warning' className='relaxed'>
      <Alert.Indicator />
      <Alert.Content>
        <Alert.Title>{t('item.deletedTitle')}</Alert.Title>
        <Alert.Description>{t('item.deletedBody')}</Alert.Description>
      </Alert.Content>
      <RestoreActionItemButton item={item} />
    </Alert>}

    <div className='relaxed'>
      <ActionItemBadges item={item} now={now} showState />
    </div>

    <div className='grid grid-cols-1 items-start gap-8 lg:grid-cols-[minmax(0,1fr)_minmax(0,20rem)]'>
      <div>
        <section className='relaxed'>
          <h2 className='compact text-lg font-semibold'>{t('item.notesHeading')}</h2>
          {isDeleted
            ? <p className='text-sm whitespace-pre-line opacity-80'>{item.notes}</p>
            : <InlineEditableText
              id='action-item-notes'
              value={notes}
              label={t('item.notesLabel')}
              placeholder={t('item.addNotes')}
              isMultiline
              maxLength={NOTES_MAX_CHARACTERS}
              className='text-sm whitespace-pre-line opacity-80'
              onChange={(value) => editor.form.setValue('notes', value, { shouldDirty: true })}
            />}
        </section>
        <ActionItemLinks item={item} />
        <ActionItemManagerAssignment item={item} />
        {!isDeleted && <MeetingReviewLink itemId={item.id} />}
        {!isDeleted && <SameMeetingPanel itemId={item.id} />}
        {!isDeleted && <MeetingThreadLink itemId={item.id} />}
        {!isDeleted && <ActionItemReply itemId={item.id} />}
        {!isDeleted && <ActionItemFollowUp item={item} />}
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
        <ActionItemDetailsPanel item={item} form={editor.form} />
      </aside>
    </div>

    {snoozeDateDialog.isOpen && <SnoozeDateModal
      state={snoozeDateDialog}
      onSnooze={(until) => itemActions.snooze(item, until)}
    />}
  </Page>
}
