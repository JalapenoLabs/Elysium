// Copyright © 2026 Jalapeno Labs

// Misc
import { apiClient } from '../index'

// One repository a project declares for its workers, in the order the project lists them.
export type DeclaredRepository = {
  id: string
  url: string
  // The branch work starts from; the remote's default when null.
  baseBranch: string | null
  // Bash, run as the agent in the repository's directory once it is cloned.
  setupScript: string | null
}

// Mirrors `WorkerSetupResponse` in api/src/routes/v1/projects/worker_setup.rs.
export type WorkerSetup = {
  // Bash, run as root in every worker's container before anything is cloned.
  toolsSetupScript: string | null
  repositories: DeclaredRepository[]
}

// The whole setup, replacing what the project had, in order. A blank base branch or script is
// none; a script is otherwise kept exactly as written.
export type WorkerSetupRequest = {
  toolsSetupScript: string | null
  repositories: {
    url: string
    baseBranch: string | null
    setupScript: string | null
  }[]
}

type WorkerSetupResponse = {
  workerSetup: WorkerSetup
}

export function getProjectWorkerSetupKey(projectId: string) {
  return `v1/projects/${projectId}/worker-setup`
}

export function getProjectWorkerSetup(projectId: string) {
  return apiClient
    .get(getProjectWorkerSetupKey(projectId))
    .json<WorkerSetupResponse>()
}

export function replaceProjectWorkerSetup(projectId: string, body: WorkerSetupRequest) {
  return apiClient
    .put(getProjectWorkerSetupKey(projectId), { json: body })
    .json<WorkerSetupResponse>()
}
