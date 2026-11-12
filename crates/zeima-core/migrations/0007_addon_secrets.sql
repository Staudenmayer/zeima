-- Names of the keychain entries an addon owns (its secrets and connected
-- OAuth accounts). The values live in the OS keychain, never here; the
-- names let the app show connections and delete the entries on uninstall.
-- Device-local like `addon`.

CREATE TABLE addon_secret (
    addon_id TEXT NOT NULL REFERENCES addon (id) ON DELETE CASCADE,
    name     TEXT NOT NULL,
    PRIMARY KEY (addon_id, name)
);
