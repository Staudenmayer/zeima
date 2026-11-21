CREATE UNLOGGED TABLE "presence" (
	"org_id" uuid NOT NULL,
	"user_id" uuid NOT NULL,
	"state" text NOT NULL,
	"project_id" uuid,
	"since" bigint NOT NULL,
	"seen" bigint NOT NULL,
	CONSTRAINT "presence_org_id_user_id_pk" PRIMARY KEY("org_id","user_id")
);
--> statement-breakpoint
CREATE UNLOGGED TABLE "replica" (
	"id" text PRIMARY KEY NOT NULL,
	"heartbeat_at" timestamp with time zone DEFAULT now() NOT NULL
);
--> statement-breakpoint
CREATE UNLOGGED TABLE "ws_conn" (
	"replica_id" text NOT NULL,
	"user_id" uuid NOT NULL,
	"n" integer NOT NULL,
	CONSTRAINT "ws_conn_replica_id_user_id_pk" PRIMARY KEY("replica_id","user_id")
);
--> statement-breakpoint
ALTER TABLE "presence" ADD CONSTRAINT "presence_org_id_organization_id_fk" FOREIGN KEY ("org_id") REFERENCES "public"."organization"("id") ON DELETE cascade ON UPDATE no action;--> statement-breakpoint
ALTER TABLE "presence" ADD CONSTRAINT "presence_user_id_kc_user_id_fk" FOREIGN KEY ("user_id") REFERENCES "public"."kc_user"("id") ON DELETE cascade ON UPDATE no action;--> statement-breakpoint
CREATE INDEX "presence_user" ON "presence" USING btree ("user_id");--> statement-breakpoint
CREATE INDEX "ws_conn_user" ON "ws_conn" USING btree ("user_id");
--> statement-breakpoint
-- Owner only: the API checks membership before it touches these (see src/plugins/presence.ts); the restricted role gets nothing.
REVOKE ALL ON "presence", "replica", "ws_conn" FROM zeima_app;--> statement-breakpoint
ALTER TABLE "presence" ENABLE ROW LEVEL SECURITY;--> statement-breakpoint
ALTER TABLE "replica" ENABLE ROW LEVEL SECURITY;--> statement-breakpoint
ALTER TABLE "ws_conn" ENABLE ROW LEVEL SECURITY;
