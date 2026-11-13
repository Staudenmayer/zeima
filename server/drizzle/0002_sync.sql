CREATE SEQUENCE "public"."sync_seq" INCREMENT BY 1 MINVALUE 1 MAXVALUE 9223372036854775807 START WITH 1 CACHE 1;--> statement-breakpoint
CREATE TABLE "sync_row" (
	"workspace_id" uuid NOT NULL,
	"table_name" text NOT NULL,
	"row_id" uuid NOT NULL,
	"fields" jsonb DEFAULT '{}'::jsonb NOT NULL,
	"deleted" text,
	"seq" bigint DEFAULT nextval('sync_seq') NOT NULL,
	CONSTRAINT "sync_row_workspace_id_table_name_row_id_pk" PRIMARY KEY("workspace_id","table_name","row_id")
);
--> statement-breakpoint
ALTER TABLE "sync_row" ADD CONSTRAINT "sync_row_workspace_id_workspace_id_fk" FOREIGN KEY ("workspace_id") REFERENCES "public"."workspace"("id") ON DELETE no action ON UPDATE no action;--> statement-breakpoint
CREATE INDEX "sync_row_seq" ON "sync_row" USING btree ("seq");