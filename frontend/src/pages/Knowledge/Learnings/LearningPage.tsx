// Copyright © 2026 Jalapeno Labs

import type { Learning } from '../../../api/routes/learningRoutes'

// Core
import { useState } from 'react'
import { useParams } from 'react-router'
import { useTranslation } from 'react-i18next'
import { mutate } from 'swr'

// Redux
import { useAppDispatch, useAppSelector } from '../../../store/hooks'
import { learningUpserted, selectLearningById } from '../../../store/learningsSlice'

// User interface
import { Alert, Button, Chip, Link, Spinner } from '@heroui/react'
import { LuRotateCcw, LuTrash2 } from 'react-icons/lu'
import { Page } from '../../../layout/page/Page'
import { LearningForm } from './LearningForm'
import { LearningHistory } from './LearningHistory'

// Misc
import { formatDateTime } from '../../../format/dates'
import { deleteLearning, restoreLearning } from '../../../api/routes/learningRoutes'
import { useLearningCategories } from '../../../hooks/useLearningCategories'
import { useLearningLoader } from '../../../hooks/useServerData'
import { useUserNames } from '../../../hooks/useUserNames'
import { UrlTree } from '../../../urls'
import { toast } from '../../../toast/toast'

// `/knowledge/learnings/:learningId`: one learning, corrected in place and saved from the header,
// with its history below. Deleting is soft, so it happens at once with Undo in its toast; a
// deleted learning reads as it was, with Restore.
export function LearningPage() {
  const { t } = useTranslation([ 'learnings', 'knowledge', 'common' ])
  const dispatch = useAppDispatch()
  const { learningId = '' } = useParams()
  const status = useLearningLoader(learningId)
  const learning = useAppSelector((state) => selectLearningById(state, learningId))
  const userNames = useUserNames()
  const { labels: categoryLabels } = useLearningCategories()
  const [ isChanging, setIsChanging ] = useState(false)

  async function restore(target: Learning) {
    setIsChanging(true)
    try {
      const response = await restoreLearning(target.id)
      dispatch(learningUpserted(response.learning))
      toast.success(t('toasts.restored'))
    }
    catch (error) {
      console.debug('LearningPage could not restore the learning', { error, learningId: target.id })
      toast.danger(t('common:errors.unexpected'))
    }
    finally {
      setIsChanging(false)
    }
  }

  async function remove(target: Learning) {
    setIsChanging(true)
    try {
      await deleteLearning(target.id)
      // The delete answers nothing, so the learning is read again to show it deleted.
      await mutate(`v1/learnings/${target.id}`)
      toast.success(t('toasts.deleted'), {
        action: {
          label: t('toasts.undo'),
          onPress: () => void restore(target),
        },
      })
    }
    catch (error) {
      console.debug('LearningPage could not delete the learning', { error, learningId: target.id })
      toast.danger(t('common:errors.unexpected'))
    }
    finally {
      setIsChanging(false)
    }
  }

  if (!learning) {
    if (status === 'loading') {
      return <div className='grid place-items-center py-16'>
        <Spinner />
      </div>
    }
    return <div className='container py-10 text-center text-sm'>
      <p className='compact opacity-70'>{t('page.notFound')}</p>
      <Link href={UrlTree.knowledgeLearnings} className='text-link'>{t('page.backToList')}</Link>
    </div>
  }

  const savedOn = formatDateTime(learning.createdAt)
  const savedBy = learning.agent
    ? 'page.savedByAgent'
    : 'page.savedBy'

  return <Page
    title={learning.title}
    parents={[
      { label: t('knowledge:title'), href: UrlTree.knowledge },
      { label: t('title'), href: UrlTree.knowledgeLearnings },
    ]}
    primaryAction={learning.deletedAt && <Button
      variant='outline'
      isPending={isChanging}
      onPress={() => restore(learning)}
    >
      <LuRotateCcw className='size-4' aria-hidden />
      <span>{t('page.restore')}</span>
    </Button>}
    // Delete, like every destructive action, is in the Actions menu rather than a bare button.
    actions={learning.deletedAt
      ? undefined
      : [{
        id: 'delete',
        label: t('page.delete'),
        icon: LuTrash2,
        isDanger: true,
        isPending: isChanging,
        onAction: () => void remove(learning),
      }]}
  >
    <div className='relaxed flex flex-wrap items-center gap-2 text-xs'>
      <Chip size='sm' variant='soft'>{categoryLabels[learning.category] ?? learning.category}</Chip>
      <span className='opacity-70'>{
        t(savedBy, {
          name: userNames[learning.createdBy] ?? t('common:table.someone'),
          agent: learning.agent?.name ?? t('agent.unnamed'),
          date: savedOn,
        })
      }</span>
    </div>

    {learning.deletedAt
      ? <div className='relaxed'>
        <Alert status='warning' className='relaxed'>
          <Alert.Indicator />
          <Alert.Content>
            <Alert.Description>{
              t('page.deleted', { date: formatDateTime(learning.deletedAt) })
            }</Alert.Description>
          </Alert.Content>
        </Alert>
        <p className='compact text-sm whitespace-pre-line'>{learning.body}</p>
        <p className='text-sm whitespace-pre-line opacity-70'>{learning.evidence}</p>
      </div>
      : <div className='relaxed'>
        <LearningForm
          // Remount whenever the learning changes, here, in another tab, or by an undo, so the
          // form always corrects what the learning says now.
          key={`${learning.id}:${learning.updatedAt}`}
          learning={learning}
        />
      </div>}

    <section className='max-w-3xl'>
      <h2 className='compact text-base font-semibold'>{t('history.title')}</h2>
      <LearningHistory learningId={learning.id} />
    </section>
  </Page>
}
