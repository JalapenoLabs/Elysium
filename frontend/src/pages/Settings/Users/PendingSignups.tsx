// Copyright © 2026 Jalapeno Labs

import type { PersonRole, User } from '../../../api/routes/userRoutes'

// Core
import { useTranslation } from 'react-i18next'

// User interface
import { PendingSignupRow } from './PendingSignupRow'

// Misc
import { PERSON_ROLES, roleLabelKeys } from './usersPresentation'

type Props = {
  users: User[]
  onApprove: (user: User, role: PersonRole) => Promise<void>
  onReject: (user: User) => void
}

// Sign-ups waiting for an admin, pinned above everyone else: approve with a role, or reject.
export function PendingSignups(props: Props) {
  const { t } = useTranslation([ 'users', 'common' ])
  const roleOptions = PERSON_ROLES.map((role) => ({ id: role, label: t(roleLabelKeys[role]) }))

  return <section className='relaxed'>
    <h2 className='compact text-xl font-semibold'>{
      t('pending.heading', { count: props.users.length })
    }</h2>
    <ul className='flex flex-col gap-2'>{
      props.users.map((user) => <PendingSignupRow
        key={user.id}
        user={user}
        roleOptions={roleOptions}
        onApprove={props.onApprove}
        onReject={props.onReject}
      />)
    }</ul>
  </section>
}
