// Applies the migrations in server/drizzle to DATABASE_URL.
import { connect, migrateDb } from '../src/db/index.ts'

const url = process.env.DATABASE_URL ?? 'postgres://zeima:zeima@localhost:5432/zeima'
const { client, db } = connect(url, { max: 1, onnotice: () => {} })
await migrateDb(db)
await client.end()
console.log('migrations applied')
