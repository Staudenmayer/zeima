CREATE TABLE "scim_token" (
	"id" uuid PRIMARY KEY DEFAULT gen_random_uuid() NOT NULL,
	"org_id" uuid NOT NULL,
	"name" text NOT NULL,
	"hash" text NOT NULL,
	"hint" text NOT NULL,
	"created_by" uuid,
	"created_at" bigint NOT NULL,
	"last_used_at" bigint,
	CONSTRAINT "scim_token_hash_unique" UNIQUE("hash")
);
--> statement-breakpoint
ALTER TABLE "scim_token" ADD CONSTRAINT "scim_token_org_id_organization_id_fk" FOREIGN KEY ("org_id") REFERENCES "public"."organization"("id") ON DELETE cascade ON UPDATE no action;--> statement-breakpoint
CREATE INDEX "scim_token_org" ON "scim_token" USING btree ("org_id");