CREATE TABLE "audit_log" (
	"id" bigserial PRIMARY KEY NOT NULL,
	"org_id" uuid,
	"actor_id" uuid,
	"action" text NOT NULL,
	"target" text,
	"detail" jsonb DEFAULT '{}'::jsonb NOT NULL,
	"at" bigint NOT NULL
);
--> statement-breakpoint
CREATE TABLE "device" (
	"id" uuid PRIMARY KEY NOT NULL,
	"user_id" uuid NOT NULL,
	"name" text NOT NULL,
	"platform" text NOT NULL,
	"app_version" text,
	"created_at" bigint NOT NULL,
	"last_seen_at" bigint
);
--> statement-breakpoint
CREATE TABLE "entitlement" (
	"id" uuid PRIMARY KEY NOT NULL,
	"user_id" uuid,
	"org_id" uuid,
	"plan" text NOT NULL,
	"seats" integer,
	"features" jsonb DEFAULT '{}'::jsonb NOT NULL,
	"valid_until" bigint,
	"updated_at" bigint NOT NULL,
	CONSTRAINT "entitlement_subject" CHECK (("entitlement"."user_id" IS NULL) <> ("entitlement"."org_id" IS NULL))
);
--> statement-breakpoint
CREATE TABLE "kc_group" (
	"id" uuid PRIMARY KEY NOT NULL,
	"org_id" uuid NOT NULL,
	"path" text NOT NULL
);
--> statement-breakpoint
CREATE TABLE "kc_group_member" (
	"group_id" uuid NOT NULL,
	"user_id" uuid NOT NULL,
	CONSTRAINT "kc_group_member_group_id_user_id_pk" PRIMARY KEY("group_id","user_id")
);
--> statement-breakpoint
CREATE TABLE "kc_org_member" (
	"org_id" uuid NOT NULL,
	"user_id" uuid NOT NULL,
	"role" text NOT NULL,
	CONSTRAINT "kc_org_member_org_id_user_id_pk" PRIMARY KEY("org_id","user_id"),
	CONSTRAINT "kc_org_member_role" CHECK ("kc_org_member"."role" IN ('owner', 'admin', 'manager', 'member'))
);
--> statement-breakpoint
CREATE TABLE "kc_user" (
	"id" uuid PRIMARY KEY NOT NULL,
	"email" text,
	"display_name" text,
	"locale" text,
	"active" boolean DEFAULT true NOT NULL,
	"last_seen_at" bigint
);
--> statement-breakpoint
CREATE TABLE "organization" (
	"id" uuid PRIMARY KEY NOT NULL,
	"name" text NOT NULL,
	"created_at" bigint NOT NULL
);
--> statement-breakpoint
CREATE TABLE "pause" (
	"id" uuid PRIMARY KEY NOT NULL,
	"workspace_id" uuid NOT NULL,
	"user_id" uuid NOT NULL,
	"start_at" bigint NOT NULL,
	"end_at" bigint,
	"kind" text NOT NULL,
	"rule_id" text,
	"accepted" boolean,
	"resume_entry_id" uuid,
	"created_at" bigint NOT NULL,
	"updated_at" bigint NOT NULL,
	"deleted_at" bigint,
	"hlc" text NOT NULL,
	"origin_device" uuid NOT NULL,
	CONSTRAINT "pause_range_ok" CHECK ("pause"."end_at" IS NULL OR "pause"."end_at" >= "pause"."start_at")
);
--> statement-breakpoint
CREATE TABLE "project" (
	"id" uuid PRIMARY KEY NOT NULL,
	"workspace_id" uuid NOT NULL,
	"name" text NOT NULL,
	"alias" text,
	"color" text NOT NULL,
	"parent_id" uuid,
	"client" text,
	"budget_minutes" integer,
	"budget_period" text DEFAULT 'total' NOT NULL,
	"budget_warn_pct" integer DEFAULT 80 NOT NULL,
	"billable_default" boolean DEFAULT false NOT NULL,
	"status" text DEFAULT 'active' NOT NULL,
	"created_at" bigint NOT NULL,
	"updated_at" bigint NOT NULL,
	"deleted_at" bigint,
	"hlc" text NOT NULL,
	"origin_device" uuid NOT NULL,
	CONSTRAINT "project_budget_period" CHECK ("project"."budget_period" IN ('total', 'monthly', 'weekly')),
	CONSTRAINT "project_status" CHECK ("project"."status" IN ('active', 'paused', 'archived'))
);
--> statement-breakpoint
CREATE TABLE "tag" (
	"id" uuid PRIMARY KEY NOT NULL,
	"workspace_id" uuid NOT NULL,
	"name" text NOT NULL,
	"color" text NOT NULL,
	"project_id" uuid,
	"group_name" text,
	"created_at" bigint NOT NULL,
	"updated_at" bigint NOT NULL,
	"deleted_at" bigint,
	"hlc" text NOT NULL,
	"origin_device" uuid NOT NULL
);
--> statement-breakpoint
CREATE TABLE "time_entry" (
	"id" uuid PRIMARY KEY NOT NULL,
	"workspace_id" uuid NOT NULL,
	"user_id" uuid NOT NULL,
	"project_id" uuid,
	"start_at" bigint NOT NULL,
	"end_at" bigint,
	"tz" text NOT NULL,
	"note" text DEFAULT '' NOT NULL,
	"billable" boolean DEFAULT false NOT NULL,
	"source" text NOT NULL,
	"source_ref" text,
	"locked" boolean DEFAULT false NOT NULL,
	"created_at" bigint NOT NULL,
	"updated_at" bigint NOT NULL,
	"deleted_at" bigint,
	"hlc" text NOT NULL,
	"origin_device" uuid NOT NULL,
	CONSTRAINT "time_entry_range_ok" CHECK ("time_entry"."end_at" IS NULL OR "time_entry"."end_at" >= "time_entry"."start_at")
);
--> statement-breakpoint
CREATE TABLE "time_entry_tag" (
	"entry_id" uuid NOT NULL,
	"tag_id" uuid NOT NULL,
	CONSTRAINT "time_entry_tag_entry_id_tag_id_pk" PRIMARY KEY("entry_id","tag_id")
);
--> statement-breakpoint
CREATE TABLE "workspace" (
	"id" uuid PRIMARY KEY NOT NULL,
	"kind" text NOT NULL,
	"owner_id" uuid,
	"org_id" uuid,
	"name" text NOT NULL,
	"settings" jsonb DEFAULT '{}'::jsonb NOT NULL,
	"created_at" bigint NOT NULL,
	"updated_at" bigint NOT NULL,
	"deleted_at" bigint,
	"hlc" text NOT NULL,
	"origin_device" uuid NOT NULL,
	CONSTRAINT "workspace_kind" CHECK ("workspace"."kind" IN ('personal', 'org')),
	CONSTRAINT "workspace_owner" CHECK (("workspace"."kind" = 'personal' AND "workspace"."owner_id" IS NOT NULL AND "workspace"."org_id" IS NULL)
          OR ("workspace"."kind" = 'org' AND "workspace"."org_id" IS NOT NULL AND "workspace"."owner_id" IS NULL))
);
--> statement-breakpoint
ALTER TABLE "audit_log" ADD CONSTRAINT "audit_log_org_id_organization_id_fk" FOREIGN KEY ("org_id") REFERENCES "public"."organization"("id") ON DELETE cascade ON UPDATE no action;--> statement-breakpoint
ALTER TABLE "device" ADD CONSTRAINT "device_user_id_kc_user_id_fk" FOREIGN KEY ("user_id") REFERENCES "public"."kc_user"("id") ON DELETE cascade ON UPDATE no action;--> statement-breakpoint
ALTER TABLE "entitlement" ADD CONSTRAINT "entitlement_user_id_kc_user_id_fk" FOREIGN KEY ("user_id") REFERENCES "public"."kc_user"("id") ON DELETE cascade ON UPDATE no action;--> statement-breakpoint
ALTER TABLE "entitlement" ADD CONSTRAINT "entitlement_org_id_organization_id_fk" FOREIGN KEY ("org_id") REFERENCES "public"."organization"("id") ON DELETE cascade ON UPDATE no action;--> statement-breakpoint
ALTER TABLE "kc_group" ADD CONSTRAINT "kc_group_org_id_organization_id_fk" FOREIGN KEY ("org_id") REFERENCES "public"."organization"("id") ON DELETE cascade ON UPDATE no action;--> statement-breakpoint
ALTER TABLE "kc_group_member" ADD CONSTRAINT "kc_group_member_group_id_kc_group_id_fk" FOREIGN KEY ("group_id") REFERENCES "public"."kc_group"("id") ON DELETE cascade ON UPDATE no action;--> statement-breakpoint
ALTER TABLE "kc_group_member" ADD CONSTRAINT "kc_group_member_user_id_kc_user_id_fk" FOREIGN KEY ("user_id") REFERENCES "public"."kc_user"("id") ON DELETE cascade ON UPDATE no action;--> statement-breakpoint
ALTER TABLE "kc_org_member" ADD CONSTRAINT "kc_org_member_org_id_organization_id_fk" FOREIGN KEY ("org_id") REFERENCES "public"."organization"("id") ON DELETE cascade ON UPDATE no action;--> statement-breakpoint
ALTER TABLE "kc_org_member" ADD CONSTRAINT "kc_org_member_user_id_kc_user_id_fk" FOREIGN KEY ("user_id") REFERENCES "public"."kc_user"("id") ON DELETE cascade ON UPDATE no action;--> statement-breakpoint
ALTER TABLE "pause" ADD CONSTRAINT "pause_workspace_id_workspace_id_fk" FOREIGN KEY ("workspace_id") REFERENCES "public"."workspace"("id") ON DELETE no action ON UPDATE no action;--> statement-breakpoint
ALTER TABLE "pause" ADD CONSTRAINT "pause_user_id_kc_user_id_fk" FOREIGN KEY ("user_id") REFERENCES "public"."kc_user"("id") ON DELETE no action ON UPDATE no action;--> statement-breakpoint
ALTER TABLE "pause" ADD CONSTRAINT "pause_resume_entry_id_time_entry_id_fk" FOREIGN KEY ("resume_entry_id") REFERENCES "public"."time_entry"("id") ON DELETE no action ON UPDATE no action;--> statement-breakpoint
ALTER TABLE "project" ADD CONSTRAINT "project_workspace_id_workspace_id_fk" FOREIGN KEY ("workspace_id") REFERENCES "public"."workspace"("id") ON DELETE no action ON UPDATE no action;--> statement-breakpoint
ALTER TABLE "tag" ADD CONSTRAINT "tag_workspace_id_workspace_id_fk" FOREIGN KEY ("workspace_id") REFERENCES "public"."workspace"("id") ON DELETE no action ON UPDATE no action;--> statement-breakpoint
ALTER TABLE "tag" ADD CONSTRAINT "tag_project_id_project_id_fk" FOREIGN KEY ("project_id") REFERENCES "public"."project"("id") ON DELETE no action ON UPDATE no action;--> statement-breakpoint
ALTER TABLE "time_entry" ADD CONSTRAINT "time_entry_workspace_id_workspace_id_fk" FOREIGN KEY ("workspace_id") REFERENCES "public"."workspace"("id") ON DELETE no action ON UPDATE no action;--> statement-breakpoint
ALTER TABLE "time_entry" ADD CONSTRAINT "time_entry_user_id_kc_user_id_fk" FOREIGN KEY ("user_id") REFERENCES "public"."kc_user"("id") ON DELETE no action ON UPDATE no action;--> statement-breakpoint
ALTER TABLE "time_entry" ADD CONSTRAINT "time_entry_project_id_project_id_fk" FOREIGN KEY ("project_id") REFERENCES "public"."project"("id") ON DELETE no action ON UPDATE no action;--> statement-breakpoint
ALTER TABLE "time_entry_tag" ADD CONSTRAINT "time_entry_tag_entry_id_time_entry_id_fk" FOREIGN KEY ("entry_id") REFERENCES "public"."time_entry"("id") ON DELETE cascade ON UPDATE no action;--> statement-breakpoint
ALTER TABLE "time_entry_tag" ADD CONSTRAINT "time_entry_tag_tag_id_tag_id_fk" FOREIGN KEY ("tag_id") REFERENCES "public"."tag"("id") ON DELETE no action ON UPDATE no action;--> statement-breakpoint
ALTER TABLE "workspace" ADD CONSTRAINT "workspace_owner_id_kc_user_id_fk" FOREIGN KEY ("owner_id") REFERENCES "public"."kc_user"("id") ON DELETE no action ON UPDATE no action;--> statement-breakpoint
ALTER TABLE "workspace" ADD CONSTRAINT "workspace_org_id_organization_id_fk" FOREIGN KEY ("org_id") REFERENCES "public"."organization"("id") ON DELETE no action ON UPDATE no action;--> statement-breakpoint
CREATE INDEX "audit_log_org_at" ON "audit_log" USING btree ("org_id","at");--> statement-breakpoint
CREATE INDEX "device_user" ON "device" USING btree ("user_id");--> statement-breakpoint
CREATE UNIQUE INDEX "kc_group_path" ON "kc_group" USING btree ("path");--> statement-breakpoint
CREATE INDEX "pause_range" ON "pause" USING btree ("user_id","start_at") WHERE "pause"."deleted_at" IS NULL;--> statement-breakpoint
CREATE UNIQUE INDEX "pause_one_open" ON "pause" USING btree ("user_id") WHERE "pause"."end_at" IS NULL AND "pause"."deleted_at" IS NULL;--> statement-breakpoint
CREATE UNIQUE INDEX "project_alias_unique" ON "project" USING btree ("workspace_id",lower("alias")) WHERE "project"."alias" IS NOT NULL AND "project"."deleted_at" IS NULL;--> statement-breakpoint
CREATE UNIQUE INDEX "tag_name_unique" ON "tag" USING btree ("workspace_id",coalesce("project_id"::text, ''),lower("name")) WHERE "tag"."deleted_at" IS NULL;--> statement-breakpoint
CREATE INDEX "time_entry_range" ON "time_entry" USING btree ("user_id","start_at") WHERE "time_entry"."deleted_at" IS NULL;--> statement-breakpoint
CREATE UNIQUE INDEX "time_entry_one_running" ON "time_entry" USING btree ("user_id") WHERE "time_entry"."end_at" IS NULL AND "time_entry"."deleted_at" IS NULL;