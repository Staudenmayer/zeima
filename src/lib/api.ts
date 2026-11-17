// A typed client for the Zeima API, from the contract (`packages/api-contract`): the path, the
// parameters, the body and the answer of every call are checked against `openapi.json`.
//
// The desktop never talks to the server from the webview: the token lives in the app's backend.
// So the client's `fetch` hands each request to the `org_request` command, which adds the token,
// sends it and answers with the body. An error answer (the server's problem details, or being
// signed out) is thrown as an `AppError`, as with every other command, so a call either returns
// its data or throws.

import createClient from 'openapi-fetch'
import type { paths } from '@zeima/api-contract/schema'
import { commands } from '@/bindings'
import { call } from '@/lib/ipc'

/** Only the path and the query are used; the backend knows where the server is. */
const ORIGIN = 'https://api.zeima.invalid'

const client = createClient<paths>({
  baseUrl: ORIGIN,
  fetch: async (request) => {
    const url = new URL(request.url)
    const body = request.method === 'GET' || request.method === 'DELETE' ? '' : await request.text()
    const text = await call(commands.orgRequest(request.method, url.pathname + url.search, body || null))
    return text === null
      ? new Response(null, { status: 204 })
      : new Response(text, { status: 200, headers: { 'content-type': 'application/json' } })
  },
})

/**
 * The typed client: `api.GET('/v1/orgs/{orgId}/members', { params: { path: { orgId } } })`.
 * Pass the result to `ok()` for the data.
 */
export const api = client

/** The data of a call that went through (a failed call has thrown). `undefined` when the server sends no content. */
export const ok = <T>(result: { data?: T }): T => result.data as T

type Operation<P extends keyof paths, M extends string> = M extends keyof paths[P] ? paths[P][M] : never
type Json<T> = T extends { content: { 'application/json': infer U } } ? U : never
/** What a call answers with when it succeeds. */
export type Reply<P extends keyof paths, M extends string> = Operation<P, M> extends { responses: infer R }
  ? R extends { 200: infer A }
    ? Json<A>
    : R extends { 201: infer B }
      ? Json<B>
      : undefined
  : never
