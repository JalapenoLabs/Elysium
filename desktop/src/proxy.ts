// Copyright © 2026 Jalapeno Labs

// Core
import http, { type IncomingMessage, type ServerResponse } from 'node:http'
import https from 'node:https'

// Utility
import { readFile } from 'node:fs/promises'
import { join, normalize } from 'node:path'
import type { AddressInfo } from 'node:net'

// Misc
import { activeServerSync } from './config.js'
import { RENDERER_DIRECTORY } from './constants.js'

// Request paths forwarded to the active remote instance; everything else is served
// from the packaged renderer. Serving the app and its API from one local origin
// keeps the session cookie first-party (no CORS, no SameSite=None) and streams SSE
// through unchanged, exactly like the Vite dev proxy, but with a runtime-swappable
// target so the client can point at any server.
const REMOTE_EXACT = ['/api', '/auth']
const REMOTE_PREFIXES = ['/api/', '/auth/']

// How long a server probe waits before giving up, in milliseconds.
const PROBE_TIMEOUT_MS = 8000

const CONTENT_TYPES: Record<string, string> = {
  '.html': 'text/html; charset=utf-8',
  '.js': 'text/javascript; charset=utf-8',
  '.mjs': 'text/javascript; charset=utf-8',
  '.css': 'text/css; charset=utf-8',
  '.json': 'application/json; charset=utf-8',
  '.svg': 'image/svg+xml',
  '.png': 'image/png',
  '.jpg': 'image/jpeg',
  '.jpeg': 'image/jpeg',
  '.gif': 'image/gif',
  '.ico': 'image/x-icon',
  '.woff': 'font/woff',
  '.woff2': 'font/woff2',
  '.wasm': 'application/wasm',
}

// The public identity an Elysium instance reports (GET /api/v1/instance).
export type InstanceInfo = {
  name: string | null
  claimed: boolean
  version: string
}

/**
 * Starts the local reverse proxy on a kernel-assigned loopback port and resolves
 * to its origin (`http://127.0.0.1:PORT`).
 *
 * The renderer loads from this origin; `/api`, `/auth`, and the realtime stream are
 * forwarded to whichever instance is active at request time, so switching servers
 * takes effect on the next window load. Everything else is served from the packaged
 * renderer directory.
 */
export function startConnectionProxy(): Promise<string> {
  const server = http.createServer(handle)
  return new Promise((resolve, reject) => {
    server.once('error', reject)
    server.listen(0, '127.0.0.1', () => {
      const { port } = server.address() as AddressInfo
      resolve(`http://127.0.0.1:${port}`)
    })
  })
}

function handle(req: IncomingMessage, res: ServerResponse): void {
  const url = req.url ?? '/'
  if (isRemotePath(url)) {
    forwardToRemote(req, res)
  }
  else {
    void serveStatic(url.split('?')[0], res)
  }
}

function isRemotePath(url: string): boolean {
  const path = url.split('?')[0]
  return REMOTE_EXACT.includes(path) || REMOTE_PREFIXES.some((prefix) => path.startsWith(prefix))
}

// Forwards an API/auth/realtime request to the active instance, streaming the
// response back (SSE included) and rebinding any Set-Cookie to the local origin.
function forwardToRemote(req: IncomingMessage, res: ServerResponse): void {
  const active = activeServerSync()
  if (!active) {
    res.writeHead(502, { 'content-type': 'text/plain' }).end('No Elysium instance selected')
    return
  }

  let target: URL
  try {
    target = new URL(req.url ?? '/', active.url)
  }
  catch {
    res.writeHead(502, { 'content-type': 'text/plain' }).end('Invalid instance URL')
    return
  }

  const client = target.protocol === 'https:' ? https : http
  const upstream = client.request(
    target,
    { method: req.method, headers: { ...req.headers, host: target.host } },
    (proxyRes) => {
      const headers = { ...proxyRes.headers }
      const setCookie = proxyRes.headers['set-cookie']
      if (setCookie) {
        // The remote scoped the cookie to its own host; rebind it to the local
        // origin (host-only) so the renderer on 127.0.0.1 stores and returns it.
        headers['set-cookie'] = setCookie.map(stripCookieDomain)
      }
      res.writeHead(proxyRes.statusCode ?? 502, headers)
      proxyRes.pipe(res)
    },
  )
  upstream.on('error', () => {
    if (res.headersSent) {
      res.end()
    }
    else {
      res.writeHead(502, { 'content-type': 'text/plain' }).end('Cannot reach the Elysium instance')
    }
  })
  req.pipe(upstream)
}

// Drops the Domain attribute from a Set-Cookie so it binds to the local origin,
// leaving HttpOnly, Secure, SameSite, Path, and expiry intact.
function stripCookieDomain(cookie: string): string {
  return cookie
    .split(';')
    .filter((part) => !/^\s*domain=/i.test(part))
    .join(';')
}

// Serves a file from the packaged renderer, falling back to index.html so
// client-side routes resolve (SPA history routing).
async function serveStatic(path: string, res: ServerResponse): Promise<void> {
  const safe = normalize(decodeURIComponent(path)).replace(/^([/\\]|\.\.[/\\])+/, '')
  const file = join(RENDERER_DIRECTORY, safe || 'index.html')
  try {
    const body = await readFile(file)
    res.writeHead(200, { 'content-type': contentTypeFor(file) }).end(body)
  }
  catch {
    try {
      const index = await readFile(join(RENDERER_DIRECTORY, 'index.html'))
      res.writeHead(200, { 'content-type': 'text/html; charset=utf-8' }).end(index)
    }
    catch {
      res.writeHead(404, { 'content-type': 'text/plain' }).end('Not found')
    }
  }
}

function contentTypeFor(file: string): string {
  const dot = file.lastIndexOf('.')
  const ext = dot >= 0 ? file.slice(dot).toLowerCase() : ''
  return CONTENT_TYPES[ext] ?? 'application/octet-stream'
}

/**
 * Probes a candidate server's public identity (`GET /api/v1/instance`).
 *
 * Used to validate that an address is a reachable Elysium instance before it is
 * saved or selected, and to show whose instance it is. Rejects on a network error,
 * a non-2xx status, a timeout, or a body that is not the expected shape.
 */
export function probeInstance(url: string): Promise<InstanceInfo> {
  const target = new URL('/api/v1/instance', url)
  const client = target.protocol === 'https:' ? https : http
  return new Promise((resolve, reject) => {
    const request = client.get(target, (response) => {
      const status = response.statusCode ?? 0
      if (status < 200 || status >= 300) {
        response.resume()
        reject(new Error(`instance responded ${status}`))
        return
      }
      let body = ''
      response.setEncoding('utf8')
      response.on('data', (chunk) => {
        body += chunk
      })
      response.on('end', () => {
        try {
          const info = JSON.parse(body) as InstanceInfo
          resolve(info)
        }
        catch {
          reject(new Error('not an Elysium instance'))
        }
      })
    })
    request.on('error', reject)
    request.setTimeout(PROBE_TIMEOUT_MS, () => {
      request.destroy(new Error('timed out'))
    })
  })
}
