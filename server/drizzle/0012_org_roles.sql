CREATE TABLE "org_role" (
	"id" uuid PRIMARY KEY NOT NULL,
	"org_id" uuid NOT NULL,
	"key" text,
	"name" text NOT NULL,
	"description" text DEFAULT '' NOT NULL,
	"permissions" jsonb DEFAULT '{}'::jsonb NOT NULL
);
--> statement-breakpoint
ALTER TABLE "kc_org_member" DROP CONSTRAINT "kc_org_member_role";--> statement-breakpoint
ALTER TABLE "org_invitation" DROP CONSTRAINT "org_invitation_role";--> statement-breakpoint
ALTER TABLE "org_role" ADD CONSTRAINT "org_role_org_id_organization_id_fk" FOREIGN KEY ("org_id") REFERENCES "public"."organization"("id") ON DELETE cascade ON UPDATE no action;--> statement-breakpoint
CREATE UNIQUE INDEX "org_role_name" ON "org_role" USING btree ("org_id",lower("name"));--> statement-breakpoint
CREATE UNIQUE INDEX "org_role_key" ON "org_role" USING btree ("org_id","key") WHERE "org_role"."key" IS NOT NULL;