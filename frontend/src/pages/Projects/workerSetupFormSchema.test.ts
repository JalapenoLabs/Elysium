// Copyright © 2026 Jalapeno Labs

import type { WorkerRepositoryValue, WorkerSetupFormValues } from './workerSetupFormSchema'

// Core
import i18next from 'i18next'
import { describe, expect, it } from 'vitest'

// Misc
import { DEFAULT_LOCALE } from '../../i18n'
import {
  createWorkerSetupSchema,
  MAX_WORKER_REPOSITORIES,
  toWorkerSetupFormValues,
  toWorkerSetupRequest,
  WORKER_SCRIPT_MAX_CHARACTERS,
} from './workerSetupFormSchema'

// testSetup initializes the app's i18next instance with the en-US translations.
const schema = createWorkerSetupSchema(
  i18next.getFixedT(DEFAULT_LOCALE, 'projects'),
  i18next.getFixedT(DEFAULT_LOCALE, 'coding'),
)

function repository(overrides: Partial<WorkerRepositoryValue> = {}): WorkerRepositoryValue {
  return {
    url: 'https://github.com/JalapenoLabs/Elysium.git',
    baseBranch: '',
    setupScript: '',
    ...overrides,
  }
}

function values(overrides: Partial<WorkerSetupFormValues> = {}): WorkerSetupFormValues {
  return {
    toolsSetupScript: '',
    repositories: [],
    ...overrides,
  }
}

// The first message the schema answers `input` with, or null when it accepts it.
function refusal(input: WorkerSetupFormValues) {
  const parsed = schema.safeParse(input)
  return parsed.error?.issues[0]?.message ?? null
}

describe('createWorkerSetupSchema', () => {
  it('accepts a setup with no script and no repositories', () => {
    expect(refusal(values())).toBeNull()
  })

  it('takes the URL forms a coding session takes, and SSH on github.com alone', () => {
    for (const url of [
      'https://github.com/JalapenoLabs/Elysium.git',
      'http://git.example.com/team/api',
      'ssh://git@github.com/JalapenoLabs/Elysium.git',
      'git@github.com:JalapenoLabs/Elysium.git',
    ]) {
      expect(refusal(values({ repositories: [ repository({ url }) ]}))).toBeNull()
    }

    expect(refusal(values({ repositories: [ repository({ url: 'git@gitlab.com:team/api.git' }) ]})))
      .toBe('Elysium has no SSH keys. Use the repository\'s https URL.')
    expect(refusal(values({ repositories: [ repository({ url: '' }) ]})))
      .toBe('Enter a repository URL such as https://github.com/org/repo.git.')
  })

  it('refuses the same github.com repository twice, over HTTPS or SSH and in any case', () => {
    const twice = values({
      repositories: [
        repository({ url: 'https://github.com/JalapenoLabs/Elysium.git' }),
        repository({ url: 'git@github.com:jalapenolabs/elysium.git' }),
      ],
    })
    expect(refusal(twice)).toBe(
      'https://github.com/JalapenoLabs/Elysium.git and git@github.com:jalapenolabs/elysium.git are the same '
        + 'repository.',
    )
  })

  it('refuses two repositories that would clone into one directory, ignoring case', () => {
    const shared = values({
      repositories: [
        repository({ url: 'https://github.com/one/api.git' }),
        repository({ url: 'https://gitlab.com/two/API.git' }),
      ],
    })
    expect(refusal(shared)).toBe(
      'https://github.com/one/api.git and https://gitlab.com/two/API.git would both clone into API. Remove one '
        + 'of them.',
    )
  })

  it('declares at most as many repositories as a batch clones', () => {
    const many = Array.from(
      { length: MAX_WORKER_REPOSITORIES + 1 },
      (_unused, index) => repository({ url: `https://github.com/team/repository-${index}.git` }),
    )
    expect(refusal(values({ repositories: many.slice(0, MAX_WORKER_REPOSITORIES) }))).toBeNull()
    expect(refusal(values({ repositories: many }))).toBe('A project lists at most 16 repositories.')
  })

  it('holds a base branch to 255 characters once trimmed', () => {
    const atLimit = repository({ baseBranch: `  ${'b'.repeat(255)}  ` })
    expect(refusal(values({ repositories: [ atLimit ]}))).toBeNull()
    const over = repository({ baseBranch: 'b'.repeat(256) })
    expect(refusal(values({ repositories: [ over ]}))).toBe('Use 255 characters or fewer.')
  })

  it('holds a script to its limit, counted by character, and lets one of only whitespace pass', () => {
    const atLimit = '🦀'.repeat(WORKER_SCRIPT_MAX_CHARACTERS)
    expect(refusal(values({ toolsSetupScript: atLimit }))).toBeNull()
    expect(refusal(values({ toolsSetupScript: `${atLimit}x` }))).toBe('Use 20,000 characters or fewer.')

    const blank = ' '.repeat(WORKER_SCRIPT_MAX_CHARACTERS + 1)
    const blankRepository = repository({ setupScript: blank })
    expect(refusal(values({ toolsSetupScript: blank, repositories: [ blankRepository ]}))).toBeNull()

    const tooLong = repository({ setupScript: 'y'.repeat(WORKER_SCRIPT_MAX_CHARACTERS + 1) })
    expect(refusal(values({ repositories: [ tooLong ]}))).toBe('Use 20,000 characters or fewer.')
  })
})

describe('toWorkerSetupRequest', () => {
  it('trims URLs and branches, sends blanks as none, and keeps every script as written', () => {
    const parsed = schema.parse(values({
      toolsSetupScript: '  apt-get install -y protobuf-compiler\n',
      repositories: [
        repository({
          url: ' https://github.com/JalapenoLabs/Elysium.git ',
          baseBranch: ' main ',
          setupScript: '\tcd frontend && yarn install\n',
        }),
        repository({ url: 'git@github.com:JalapenoLabs/uikit.git', baseBranch: '   ', setupScript: ' \n ' }),
      ],
    }))

    expect(toWorkerSetupRequest(parsed)).toEqual({
      toolsSetupScript: '  apt-get install -y protobuf-compiler\n',
      repositories: [
        {
          url: 'https://github.com/JalapenoLabs/Elysium.git',
          baseBranch: 'main',
          setupScript: '\tcd frontend && yarn install\n',
        },
        { url: 'git@github.com:JalapenoLabs/uikit.git', baseBranch: null, setupScript: null },
      ],
    })
  })

  it('sends a tools script of only whitespace as none', () => {
    const parsed = schema.parse(values({ toolsSetupScript: '\n  \n' }))
    expect(toWorkerSetupRequest(parsed).toolsSetupScript).toBeNull()
  })
})

describe('toWorkerSetupFormValues', () => {
  it('fills what the setup leaves out with blanks, in the order it lists', () => {
    expect(toWorkerSetupFormValues({
      toolsSetupScript: null,
      repositories: [
        { id: 'one', url: 'https://github.com/o/web.git', baseBranch: null, setupScript: 'yarn install' },
        { id: 'two', url: 'https://github.com/o/api.git', baseBranch: 'develop', setupScript: null },
      ],
    })).toEqual({
      toolsSetupScript: '',
      repositories: [
        { url: 'https://github.com/o/web.git', baseBranch: '', setupScript: 'yarn install' },
        { url: 'https://github.com/o/api.git', baseBranch: 'develop', setupScript: '' },
      ],
    })
  })
})
