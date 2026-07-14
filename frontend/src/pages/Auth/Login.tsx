// Copyright © 2026 Jalapeno Labs

import type { AuthFlowState } from '../../components/auth/authFlowState'
import type { StageFlashMessage } from '../../components/auth/StageFlash'

// Core
import { useEffect, useRef, useState } from 'react'
import { Navigate, useNavigate, useSearchParams } from 'react-router'
import { useTranslation } from 'react-i18next'

// Redux
import { useAppSelector } from '../../store/hooks'

// UI
import { AnimatePresence, motion, useReducedMotion } from 'framer-motion'
import { AuthShell } from '../../components/auth/AuthShell'
import { LoginIdentifierStage } from './LoginIdentifierStage'
import { LoginPasskeyStage } from './LoginPasskeyStage'
import { LoginPasswordStage } from './LoginPasswordStage'
import { LoginTotpStage } from './LoginTotpStage'
import { LoginRecoveryStage } from './LoginRecoveryStage'

// Utility
import { HTTPError } from 'ky'

// Misc
import { useAuthMethods } from '../../auth/methods/useAuthMethods'
import { usePasskeySignIn } from '../../auth/methods/usePasskeySignIn'
import { usePasswordSignIn } from '../../auth/methods/usePasswordSignIn'
import { recoveryAuthenticate } from '../../api/routes/authRoutes'
import { resolveSession } from '../../auth/session'
import { UrlTree } from '../../urls'

type LoginStageKind = 'identifier' | 'passkey' | 'password' | 'totp' | 'recovery'

// Deliberately loose: it only catches "that can't be an email" typos locally.
// The server remains the authority on whether the identifier signs in.
const EMAIL_SHAPE = /^\S+@\S+\.\S+$/

const TOTP_SHAPE = /^\d{6}$/

// How long a local-validation flash stays on screen before it fades.
const LOCAL_FLASH_DURATION_MS = 2500

// The shared auth-method hooks surface semantic error codes; this maps them to
// the gate's copy. Unknown codes fall back to the generic failure line.
const messageKeyByErrorCode: Record<string, string> = {
  invalid: 'auth:login.failed',
  failed: 'auth:login.failed',
  rateLimited: 'auth:login.rateLimited',
  invalidCode: 'auth:login.totp.invalidCode',
}

// The anonymous entry point, staged like a title screen rather than a form (see
// docs/auth.md, "The Gate"). An invisible input owns focus, the typed value is
// the hero text, Enter advances, and method discovery decides which stage comes
// next: the native passkey ceremony, the password stage, or the TOTP second
// factor. The flows themselves live in the shared headless hooks under
// `auth/methods/`; this page only stages them. The ambient field's hue tracks
// the flow state.
export function Login() {
  const { t } = useTranslation([ 'auth', 'common' ])
  const navigate = useNavigate()
  const [ searchParams ] = useSearchParams()
  const prefersReducedMotion = useReducedMotion()
  const status = useAppSelector((state) => state.session.status)
  const instanceClaimed = useAppSelector((state) => state.session.instanceClaimed)

  // "Add account" mode (from the profile badge): an already-authenticated user
  // is deliberately here to sign into another account, so the gate renders
  // instead of redirecting home. Until multi-session support ships server-side,
  // the new sign-in replaces the current session.
  const isAddingAccount = searchParams.get('intent') === 'add'

  const authMethods = useAuthMethods()
  const passkey = usePasskeySignIn()
  const passwordSignIn = usePasswordSignIn()

  const [ stage, setStage ] = useState<LoginStageKind>('identifier')
  const [ email, setEmail ] = useState('')
  const [ password, setPassword ] = useState('')
  const [ totpCode, setTotpCode ] = useState('')
  const [ recoveryCode, setRecoveryCode ] = useState('')
  const [ isRecovering, setIsRecovering ] = useState(false)
  const [ flash, setFlash ] = useState<StageFlashMessage | null>(null)
  // Where a completed sign-in lands. Home normally; the Security Center after a
  // recovery sign-in so a fresh passkey gets enrolled immediately. The
  // authenticated guard below reads this, so the redirect can't race it.
  const [ postSignInTarget, setPostSignInTarget ] = useState<string>(UrlTree.home)

  // Local-validation flashes decay on a timer; server failures stay until the
  // next attempt. The nonce keeps repeated identical messages re-animating.
  const flashNonceRef = useRef(0)
  const flashTimerRef = useRef<ReturnType<typeof setTimeout> | undefined>(undefined)
  useEffect(() => {
    return () => clearTimeout(flashTimerRef.current)
  }, [])

  function showFlash(tone: StageFlashMessage['tone'], message: string) {
    clearTimeout(flashTimerRef.current)
    flashNonceRef.current += 1
    setFlash({ tone, message, nonce: flashNonceRef.current })
    if (tone === 'warning') {
      flashTimerRef.current = setTimeout(() => setFlash(null), LOCAL_FLASH_DURATION_MS)
    }
  }

  // On success the session probe flips `status` to authenticated and the guard
  // below carries the user into the workspace (via `postSignInTarget`).
  async function completeSignIn(destination: string = UrlTree.home) {
    await resolveSession()
    navigate(destination)
  }

  // The password machine advances on its own (password -> totp -> done); mirror
  // its progress into the gate's staging. `stage` is a dependency so the mirror
  // re-checks if the gate happens to enter the password stage while the machine
  // is already past it.
  useEffect(() => {
    if (passwordSignIn.stage === 'totp' && stage === 'password') {
      setFlash(null)
      setStage('totp')
    }
    if (passwordSignIn.stage === 'done') {
      void completeSignIn()
    }
  }, [ passwordSignIn.stage, stage ])

  // Password/TOTP failures surface as semantic codes on the hook; each new code
  // (they reset to null per attempt) becomes a red flash in the gate's copy.
  useEffect(() => {
    if (!passwordSignIn.error) {
      return
    }
    showFlash('danger', t(messageKeyByErrorCode[passwordSignIn.error] ?? 'auth:login.failed'))
  }, [ passwordSignIn.error ])

  // An authenticated user belongs in the app, not on the login screen (unless
  // they're deliberately adding an account). An unclaimed instance has no
  // accounts yet, so send the first user to first-run.
  if (status === 'authenticated' && !isAddingAccount) {
    return <Navigate to={postSignInTarget} replace />
  }
  if (instanceClaimed === false) {
    return <Navigate to={UrlTree.setup} replace />
  }

  async function runPasskeyCeremony(identifier: string) {
    setFlash(null)
    const succeeded = await passkey.start(identifier)
    if (!succeeded) {
      showFlash('danger', t('auth:login.failed'))
      return
    }
    await completeSignIn()
  }

  async function onSubmitIdentifier() {
    const trimmedEmail = email.trim()
    if (!trimmedEmail) {
      showFlash('warning', t('auth:login.emailRequired'))
      return
    }
    if (!EMAIL_SHAPE.test(trimmedEmail)) {
      showFlash('warning', t('auth:login.emailInvalid'))
      return
    }

    setFlash(null)
    const resolvedMethods = await authMethods.resolve(trimmedEmail)

    // Passkey stays the flagship: when offered it runs first, and the password
    // path remains reachable as a fallback action on the passkey stage.
    if (resolvedMethods.includes('passkey')) {
      setStage('passkey')
      await runPasskeyCeremony(trimmedEmail)
      return
    }
    if (resolvedMethods.includes('password')) {
      setStage('password')
      return
    }
    console.debug('no supported sign-in method offered', { methods: resolvedMethods })
    showFlash('danger', t('auth:login.failed'))
  }

  async function onSubmitPassword() {
    if (!password) {
      showFlash('warning', t('auth:login.password.required'))
      return
    }
    setFlash(null)
    await passwordSignIn.submitPassword(email.trim(), password)
  }

  async function onSubmitTotp() {
    if (!TOTP_SHAPE.test(totpCode.trim())) {
      showFlash('warning', t('auth:login.totp.required'))
      return
    }
    setFlash(null)
    await passwordSignIn.submitTotp(totpCode.trim())
  }

  async function onSubmitRecovery() {
    if (!recoveryCode.trim()) {
      showFlash('warning', t('auth:login.recovery.required'))
      return
    }

    setFlash(null)
    setIsRecovering(true)
    try {
      await recoveryAuthenticate(email.trim(), recoveryCode.trim())
      // The redeemed code is gone forever, so land in the Security Center where
      // a fresh passkey can be enrolled right away.
      setPostSignInTarget(UrlTree.settingsSecurity)
      await completeSignIn(UrlTree.settingsSecurity)
    }
    catch (error) {
      console.debug('recovery sign-in failed', { error })
      const isRateLimited = error instanceof HTTPError && error.response.status === 429
      showFlash('danger', t(isRateLimited ? 'auth:login.rateLimited' : 'auth:login.failed'))
    }
    finally {
      setIsRecovering(false)
    }
  }

  function backToIdentifier() {
    // The password machine restarts with the identifier: a fresh submit
    // supersedes any short-lived pending-TOTP state on the server.
    passwordSignIn.reset()
    setPassword('')
    setTotpCode('')
    setRecoveryCode('')
    setFlash(null)
    setStage('identifier')
  }

  const isBusy = authMethods.isBusy || passkey.isBusy || passwordSignIn.isBusy || isRecovering

  // The ambient field speaks the state-color grammar: orange while a request is
  // in flight, pink while the native passkey prompt wants the user's tap, red on
  // a failed attempt, calm indigo otherwise.
  let flowState: AuthFlowState = 'idle'
  if (isBusy) {
    flowState = stage === 'passkey' ? 'needsInput' : 'authenticating'
  }
  if (flash?.tone === 'danger') {
    flowState = 'error'
  }

  let stageContent = <LoginIdentifierStage
    email={email}
    flash={flash}
    onChange={setEmail}
    onSubmit={onSubmitIdentifier}
  />
  if (stage === 'passkey') {
    stageContent = <LoginPasskeyStage
      email={email.trim()}
      isCeremonyActive={isBusy}
      canUsePassword={(authMethods.methods ?? []).includes('password')}
      flash={flash}
      onRetry={() => runPasskeyCeremony(email.trim())}
      onUsePassword={() => {
        setFlash(null)
        setStage('password')
      }}
      onUseRecovery={() => {
        setFlash(null)
        setStage('recovery')
      }}
      onBack={backToIdentifier}
    />
  }
  if (stage === 'password') {
    stageContent = <LoginPasswordStage
      email={email.trim()}
      password={password}
      flash={flash}
      onChange={setPassword}
      onSubmit={onSubmitPassword}
      onBack={backToIdentifier}
    />
  }
  if (stage === 'totp') {
    stageContent = <LoginTotpStage
      email={email.trim()}
      code={totpCode}
      flash={flash}
      onChange={setTotpCode}
      onSubmit={onSubmitTotp}
    />
  }
  if (stage === 'recovery') {
    stageContent = <LoginRecoveryStage
      email={email.trim()}
      code={recoveryCode}
      flash={flash}
      onChange={setRecoveryCode}
      onSubmit={onSubmitRecovery}
      onBack={backToIdentifier}
    />
  }

  // Stage changes play as a page turn: the old stage lifts away, the new one
  // rises in. Reduced motion trades the travel for a plain crossfade.
  const travel = prefersReducedMotion ? 0 : 48

  return <AuthShell state={flowState} variant='stage'>
    <AnimatePresence mode='wait'>
      <motion.div
        key={stage}
        className='w-full'
        initial={{ opacity: 0, y: travel }}
        animate={{ opacity: 1, y: 0 }}
        exit={{ opacity: 0, y: -travel }}
        transition={{ duration: 0.35, ease: 'easeOut' }}
      >
        { stageContent }
      </motion.div>
    </AnimatePresence>
  </AuthShell>
}
