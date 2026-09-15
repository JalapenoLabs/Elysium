// Copyright © 2026 Jalapeno Labs

// Misc
import { apiClient } from '../index'

// Mirrors `ProjectResponse` in api/src/routes/v1/projects/mod.rs.
export type Project = {
  id: string
  name: string
  description: string
  createdAt: string
  updatedAt: string
}

type ListProjectsResponse = {
  projects: Project[]
}

export function listProjects() {
  return apiClient
    .get('v1/projects')
    .json<ListProjectsResponse>()
}

type CreateProjectRequest = {
  name: string
  description?: string
}

type CreateProjectResponse = {
  project: Project
}

export function createProject(body: CreateProjectRequest) {
  return apiClient
    .post('v1/projects', { json: body })
    .json<CreateProjectResponse>()
}

// Omitted fields are left unchanged.
type UpdateProjectRequest = {
  name?: string
  description?: string
}

type UpdateProjectResponse = {
  project: Project
}

export function updateProject(projectId: string, body: UpdateProjectRequest) {
  return apiClient
    .patch(`v1/projects/${projectId}`, { json: body })
    .json<UpdateProjectResponse>()
}

// Answers 409 while any coding session still belongs to the project.
export function deleteProject(projectId: string) {
  return apiClient
    .delete(`v1/projects/${projectId}`)
}
