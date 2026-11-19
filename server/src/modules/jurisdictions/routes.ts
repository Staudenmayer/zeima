// Extra country profiles of a self-hosted installation (docs/country-profiles.md). The folder
// `JURISDICTIONS_DIR` holds profile files (`<ID>.toml`); the apps download them and use them next to
// the bundled ones (a file with a bundled id replaces it). The server only serves the text: the app
// parses and checks it and ignores what is not valid. Nothing in a profile is secret.

import { readdir, readFile, stat } from 'node:fs/promises'
import { join } from 'node:path'
import type { FastifyPluginAsyncTypebox } from '@fastify/type-provider-typebox'
import { Type } from 'typebox'
import type { Config } from '../../config.ts'

const FILE = /^[A-Za-z]{2}(?:-[A-Za-z0-9]{1,8})?\.toml$/
const MAX_BYTES = 256 * 1024
const MAX_FILES = 50

export const jurisdictionRoutes: FastifyPluginAsyncTypebox<{ config: Config }> = async (app, { config }) => {
  const dir = config.jurisdictionsDir
  let cache: { at: number; value: { profiles: { file: string; source: string }[] } } | null = null

  app.get(
    '/jurisdictions',
    {
      config: { public: 'country profiles are not secret' },
      schema: {
        tags: ['discovery'],
        summary: 'Extra country profiles this installation adds',
        description: 'Profile files as TOML text. Empty unless the administrator mounted a `JURISDICTIONS_DIR`.',
        response: { 200: Type.Object({ profiles: Type.Array(Type.Object({ file: Type.String(), source: Type.String() })) }) },
      },
    },
    async (_request, reply) => {
      if (!dir) return { profiles: [] }
      reply.header('cache-control', 'public, max-age=300')
      // The folder is read at most once a minute, however often this public route is called.
      if (cache && Date.now() - cache.at < 60_000) return cache.value
      const names = (await readdir(dir).catch(() => [] as string[])).filter((n) => FILE.test(n)).sort().slice(0, MAX_FILES)
      const profiles: { file: string; source: string }[] = []
      for (const file of names) {
        const path = join(dir, file)
        if ((await stat(path).catch(() => null))?.size! > MAX_BYTES) continue
        const source = await readFile(path, 'utf8').catch(() => null)
        if (source !== null) profiles.push({ file, source })
      }
      const value = { profiles }
      cache = { at: Date.now(), value }
      return value
    },
  )
}
