CREATE TABLE "org_job" (
	"id" uuid PRIMARY KEY NOT NULL,
	"org_id" uuid NOT NULL,
	"name" text NOT NULL,
	"description" text DEFAULT '' NOT NULL
);
--> statement-breakpoint
ALTER TABLE "kc_org_member" ADD COLUMN "job_id" uuid;--> statement-breakpoint
ALTER TABLE "org_job" ADD CONSTRAINT "org_job_org_id_organization_id_fk" FOREIGN KEY ("org_id") REFERENCES "public"."organization"("id") ON DELETE cascade ON UPDATE no action;--> statement-breakpoint
CREATE UNIQUE INDEX "org_job_name" ON "org_job" USING btree ("org_id",lower("name"));--> statement-breakpoint
ALTER TABLE "kc_org_member" ADD CONSTRAINT "kc_org_member_job_id_org_job_id_fk" FOREIGN KEY ("job_id") REFERENCES "public"."org_job"("id") ON DELETE set null ON UPDATE no action;--> statement-breakpoint
-- Job titles typed in so far become the organization's job descriptions.
INSERT INTO "org_job" ("id", "org_id", "name")
SELECT gen_random_uuid(), "org_id", "title" FROM (
  SELECT DISTINCT ON ("org_id", lower(btrim("job_title"))) "org_id", btrim("job_title") AS "title"
  FROM "kc_org_member" WHERE "job_title" IS NOT NULL AND btrim("job_title") <> ''
  ORDER BY "org_id", lower(btrim("job_title")), btrim("job_title")
) t;--> statement-breakpoint
UPDATE "kc_org_member" m SET "job_id" = j."id"
FROM "org_job" j
WHERE j."org_id" = m."org_id" AND lower(j."name") = lower(btrim(m."job_title"));
