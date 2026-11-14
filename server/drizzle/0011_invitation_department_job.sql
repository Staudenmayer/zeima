ALTER TABLE "org_invitation" ADD COLUMN "group_ids" uuid[] DEFAULT '{}'::uuid[] NOT NULL;--> statement-breakpoint
ALTER TABLE "org_invitation" ADD COLUMN "job_id" uuid;--> statement-breakpoint
ALTER TABLE "org_invitation" ADD CONSTRAINT "org_invitation_job_id_org_job_id_fk" FOREIGN KEY ("job_id") REFERENCES "public"."org_job"("id") ON DELETE set null ON UPDATE no action;