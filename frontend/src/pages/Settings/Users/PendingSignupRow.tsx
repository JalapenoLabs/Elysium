// Copyright © 2026 Jalapeno Labs

import type { PickerOption } from '../../../components/MultiPicker'
import type { PersonRole, User } from '../../../api/routes/userRoutes'

// Core
import { useState } from 'react'
import { useTranslation } from 'react-i18next'

// User interface
import { Button } from '@heroui/react'
import { OptionSelect } from '../../../components/OptionSelect'

// Misc
import { PERSON_ROLES } from './usersPresentation'

const signedUpFormatter = new Intl.DateTimeFormat(undefined, { dateStyle: 'medium', timeStyle: 'short' })

type Props = {
  user: User
  roleOptions: PickerOption[]
  onApprove: (user: User, role: PersonRole) => Promise<void>
  onReject: (user: User) => void
}

// One sign-up waiting for an admin, with the role it would be approved as (Member unless
// changed).
export function PendingSignupRow(props: Props) {
  const { t } = useTranslation([ 'users', 'common' ])
  const [ role, setRole ] = useState<PersonRole>('member')
  const [ isApproving, setIsApproving ] = useState(false)

  return <li className='level flex-wrap rounded-xl border border-warning/40 bg-surface px-4 py-3'>
    <div className='min-w-0'>
      <p className='font-medium'>{props.user.name}</p>
      <p className='text-xs opacity-70'>{
        t('pending.signedUp', {
          email: props.user.email,
          date: signedUpFormatter.format(new Date(props.user.createdAt)),
        })
      }</p>
    </div>
    <div className='level-right'>
      <OptionSelect
        isLabelHidden
        className='w-36'
        label={t('pending.role')}
        options={props.roleOptions}
        value={role}
        onChange={(value) => setRole(PERSON_ROLES.find((option) => option === value) ?? 'member')}
      />
      <Button
        size='sm'
        isPending={isApproving}
        onPress={async () => {
          setIsApproving(true)
          await props.onApprove(props.user, role)
          setIsApproving(false)
        }}
      >
        <span>{t('pending.approve')}</span>
      </Button>
      <Button size='sm' variant='ghost' onPress={() => props.onReject(props.user)}>
        <span>{t('pending.reject')}</span>
      </Button>
    </div>
  </li>
}
