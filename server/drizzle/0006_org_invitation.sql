CREATE TABLE "org_invitation" (
	"id" uuid PRIMARY KEY DEFAULT gen_random_uuid() NOT NULL,
	"org_id" uuid NOT NULL,
	"email" text NOT NULL,
	"role" text NOT NULL,
	"invited_by" uuid,
	"created_at" bigint NOT NULL,
	"accepted_at" bigint,
	CONSTRAINT "org_invitation_role" CHECK ("org_invitation"."role" IN ('admin', 'manager', 'member'))
);
--> statement-breakpoint
ALTER TABLE "org_invitation" ADD CONSTRAINT "org_invitation_org_id_organization_id_fk" FOREIGN KEY ("org_id") REFERENCES "public"."organization"("id") ON DELETE cascade ON UPDATE no action;--> statement-breakpoint
CREATE UNIQUE INDEX "org_invitation_pending" ON "org_invitation" USING btree ("org_id","email") WHERE "org_invitation"."accepted_at" IS NULL;