import { Type } from 'typebox'
import { afterAll, beforeAll, describe, expect, it } from 'vitest'
import type { App } from '../src/app.ts'
import { HttpProblem } from '../src/plugins/errors.ts'
import { testApp } from './helpers.ts'

describe('problem responses', () => {
  let app: App
  beforeAll(async () => {
    app = await testApp()
    app.post(
      '/test/echo',
      { schema: { hide: true, body: Type.Object({ n: Type.Integer({ minimum: 1 }) }) } },
      async (request) => request.body,
    )
    app.get('/test/conflict', { schema: { hide: true } }, async () => {
      throw new HttpProblem(409, 'already_exists', 'the project exists')
    })
    app.get('/test/crash', { schema: { hide: true } }, async () => {
      throw new Error('secret database detail')
    })
    await app.ready()
  })
  afterAll(() => app.close())

  it('reports validation problems with their path', async () => {
    const res = await app.inject({ method: 'POST', url: '/test/echo', payload: { n: 0 } })
    expect(res.statusCode).toBe(400)
    expect(res.headers['content-type']).toMatch(/^application\/problem\+json/)
    const body = res.json()
    expect(body.code).toBe('validation_failed')
    expect(body.errors[0].path).toBe('body/n')
  })

  it('passes on HttpProblem codes', async () => {
    const res = await app.inject({ method: 'GET', url: '/test/conflict' })
    expect(res.statusCode).toBe(409)
    expect(res.json()).toEqual({
      type: 'about:blank',
      title: 'Conflict',
      status: 409,
      code: 'already_exists',
      detail: 'the project exists',
    })
  })

  it('hides details of server errors', async () => {
    const res = await app.inject({ method: 'GET', url: '/test/crash' })
    expect(res.statusCode).toBe(500)
    expect(res.json()).toEqual({ type: 'about:blank', title: 'Internal Server Error', status: 500, code: 'internal' })
  })

  it('answers unknown routes with not_found', async () => {
    const res = await app.inject({ method: 'GET', url: '/v1/nope?x=1' })
    expect(res.statusCode).toBe(404)
    expect(res.json()).toMatchObject({ code: 'not_found', detail: 'GET /v1/nope does not exist' })
  })

  it('handles malformed JSON as a client error', async () => {
    const res = await app.inject({
      method: 'POST',
      url: '/test/echo',
      headers: { 'content-type': 'application/json' },
      payload: '{',
    })
    expect(res.statusCode).toBe(400)
    expect(res.json().code).toBe('bad_request')
  })
})
