// The typed client for the Zeima API, generated from the contract (`packages/api-contract`, by
// @hey-api/openapi-ts): the path, the parameters, the body and the answer of every call are checked
// against `openapi.json`.
//
// The desktop never talks to the server from the webview: the token lives in the app's backend.
// So the client's `fetch` hands each request to the `org_request` command, which adds the token,
// sends it and answers with the body. An error answer (the server's problem details, or being
// signed out) is thrown as an `AppError`, as with every other command, so a call either returns
// its data or throws.

import { client } from '@zeima/api-contract/instance'
import { commands } from '@/bindings'
import { call } from '@/lib/ipc'

/** Only the path and the query are used; the backend knows where the server is. */
const ORIGIN = 'https://api.zeima.invalid'

client.setConfig({
  baseUrl: ORIGIN,
  throwOnError: true,
  fetch: async (input: RequestInfo | URL, init?: RequestInit) => {
    const request = input instanceof Request ? input : new Request(input, init)
    const url = new URL(request.url)
    const body = request.method === 'GET' || request.method === 'DELETE' ? '' : await request.text()
    const text = await call(commands.orgRequest(request.method, url.pathname + url.search, body || null))
    return text === null
      ? new Response(null, { status: 204 })
      : new Response(text, { status: 200, headers: { 'content-type': 'application/json' } })
  },
})

/** Every call of the contract, as a function: `getV1OrgsByOrgIdMembers({ path: { orgId } })`. Pass the result to `ok()` for the data. */
export * from '@zeima/api-contract/client'

/** The data of a call that went through (a failed call has thrown). `undefined` when the server sends no content. */
export const ok = <T>(result: { data?: T }): T => (result.data ?? undefined) as T
