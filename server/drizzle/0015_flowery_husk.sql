CREATE TABLE "org_addon_policy" (
	"org_id" uuid PRIMARY KEY NOT NULL,
	"mode" text DEFAULT 'open' NOT NULL,
	"require_signed" boolean DEFAULT false NOT NULL,
	"allowed" text[] DEFAULT '{}'::text[] NOT NULL,
	"updated_at" bigint NOT NULL
);
--> statement-breakpoint
ALTER TABLE "org_addon_policy" ADD CONSTRAINT "org_addon_policy_org_id_organization_id_fk" FOREIGN KEY ("org_id") REFERENCES "public"."organization"("id") ON DELETE cascade ON UPDATE no action;