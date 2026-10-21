// Copyright © 2026 Jalapeno Labs

import type { Key } from '@heroui/react'
import type { ElysiaHold } from '../api/routes/elysiaRoutes'

// Core
import { useState } from 'react'
import { useTranslation } from 'react-i18next'

// Redux
import { useAppDispatch, useAppSelector } from '../store/hooks'
import { selectIsAdmin } from '../store/authSlice'
import { elysiaPanelToggled, elysiaUpdated, selectElysia } from '../store/elysiaSlice'

// User interface
import { Button, Tabs, toast, Tooltip } from '@heroui/react'
import { LuX } from 'react-icons/lu'
import { ElysiaActivity } from './ElysiaActivity'
import { ElysiaConversation } from './ElysiaConversation'
import { ElysiaHoursDialog } from './ElysiaHoursDialog'
import { ElysiaQuestionCard } from './ElysiaQuestionCard'
import { ElysiaWorkList } from './ElysiaWorkList'

// Misc
import { updateElysiaSettings } from '../api/routes/elysiaRoutes'
import { holdLabelKeys, lightDotClassNames, lightLabelKeys, lightTextClassNames } from './presentation'

type View = 'conversation' | 'activity'

// Elysia beside the page: her status, what holds her back, her questions, what she is
// working on, and the conversation or her activity. It sits in the app shell rather than an
// overlay, so the page stays usable while she is open.
export function ElysiaPanel() {
  const { t, i18n } = useTranslation('elysia')
  const dispatch = useAppDispatch()
  const elysia = useAppSelector(selectElysia)
  const isAdmin = useAppSelector(selectIsAdmin)
  const [ view, setView ] = useState<View>('conversation')
  const [ isSaving, setIsSaving ] = useState(false)
  const [ isHoursOpen, setIsHoursOpen ] = useState(false)
  const timeFormatter = new Intl.DateTimeFormat(i18n.language, { dateStyle: 'medium', timeStyle: 'short' })

  async function changeSettings(body: Parameters<typeof updateElysiaSettings>[0]) {
    setIsSaving(true)
    try {
      const response = await updateElysiaSettings(body)
      dispatch(elysiaUpdated(response.elysia))
    }
    catch (error) {
      console.debug('ElysiaPanel failed to change her settings', { error, body })
      toast.danger(t('panel.settingsFailed'))
    }
    finally {
      setIsSaving(false)
    }
  }

  function holdText(hold: ElysiaHold) {
    if (hold.kind === 'usage-limit') {
      return t(holdLabelKeys[hold.kind], { time: timeFormatter.format(new Date(hold.until)) })
    }
    return t(holdLabelKeys[hold.kind])
  }

  const light = elysia?.light ?? 'done'

  return <aside
    aria-label={t('panel.label')}
    // AppShell decides the panel's width and placement; the panel fills what it is given.
    className='flex h-full w-full flex-col border-l border-separator bg-sidebar px-4 py-3'
  >
    <header className='compact level'>
      <div className='level-left gap-2'>
        <span className={`block size-3 rounded-full ${lightDotClassNames[light]}`} aria-hidden />
        <div>
          <h2 className='text-base font-semibold'>{t('name')}</h2>
          <p className={`text-xs ${lightTextClassNames[light]}`}>{t(lightLabelKeys[light])}</p>
        </div>
      </div>
      <div className='level-right gap-1'>
        {isAdmin && elysia && <Button size='sm' variant='ghost' onPress={() => setIsHoursOpen(true)}>
          <span>{t('panel.hours')}</span>
        </Button>}
        {isAdmin && elysia && <Button
          size='sm'
          variant='ghost'
          isPending={isSaving}
          onPress={() => void changeSettings({ paused: !elysia.paused })}
        >
          <span>{elysia.paused
            ? t('panel.resume')
            : t('panel.pause')}</span>
        </Button>}
        <Tooltip delay={300}>
          <Button
            isIconOnly
            size='sm'
            variant='ghost'
            aria-label={t('panel.close')}
            onPress={() => dispatch(elysiaPanelToggled(false))}
          >
            <LuX className='size-4' aria-hidden />
          </Button>
          <Tooltip.Content>
            <span>{t('panel.close')}</span>
          </Tooltip.Content>
        </Tooltip>
      </div>
    </header>

    {elysia?.hold && <div className='compact level gap-2 rounded-xl bg-surface p-3 text-xs'>
      <span className='opacity-80'>{holdText(elysia.hold)}</span>
      {isAdmin && elysia.hold.kind === 'usage-limit' && <Button
        size='sm'
        variant='secondary'
        isPending={isSaving}
        onPress={() => void changeSettings({ resumeUsage: true })}
      >
        <span>{t('panel.resumeUsage')}</span>
      </Button>}
    </div>}

    {Boolean(elysia?.openQuestions.length) && <section className='compact'>
      <h3 className='compact text-xs font-semibold uppercase opacity-60'>{t('questions.heading')}</h3>
      {elysia?.openQuestions.map((question) => <ElysiaQuestionCard key={question.id} question={question} />)}
    </section>}

    {Boolean(elysia?.work.length) && <section className='compact max-h-[30%] overflow-y-auto'>
      <h3 className='compact text-xs font-semibold uppercase opacity-60'>{t('work.heading')}</h3>
      <ElysiaWorkList work={elysia?.work ?? []} />
    </section>}

    <Tabs
      className='compact'
      variant='secondary'
      selectedKey={view}
      onSelectionChange={(key: Key) => setView(key === 'activity'
        ? 'activity'
        : 'conversation')}
    >
      <Tabs.ListContainer>
        <Tabs.List aria-label={t('tabs.label')}>
          <Tabs.Tab id='conversation'>
            <span>{t('tabs.conversation')}</span>
            <Tabs.Indicator />
          </Tabs.Tab>
          <Tabs.Tab id='activity'>
            <span>{t('tabs.activity')}</span>
            <Tabs.Indicator />
          </Tabs.Tab>
        </Tabs.List>
      </Tabs.ListContainer>
    </Tabs>

    {view === 'conversation'
      ? <ElysiaConversation activeTurns={elysia?.activeTurns ?? []} />
      : <ElysiaActivity />}

    {/* Mounted only while open, so it starts from the hours as they are now. */}
    {isHoursOpen && elysia && <ElysiaHoursDialog
      isOpen
      onOpenChange={setIsHoursOpen}
      availability={elysia.availability}
    />}
  </aside>
}
