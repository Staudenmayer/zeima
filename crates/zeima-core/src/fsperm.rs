//! Files that hold a person's working time are theirs alone: on Unix the data folder and the backups are owner-only
//! (`0700` for folders, `0600` for files), whatever the umask says. Windows keeps its per-user profile ACLs, which
//! already exclude other accounts.

use std::io;
use std::path::Path;

/// Creates the folder (and parents) if needed and makes it owner-only.
pub fn private_dir(path: &Path) -> io::Result<()> {
    std::fs::create_dir_all(path)?;
    restrict(path, 0o700)
}

/// Makes an existing file owner-only; a file that is not there is left alone.
pub fn private_file(path: &Path) -> io::Result<()> {
    if path.exists() {
        restrict(path, 0o600)
    } else {
        Ok(())
    }
}

#[cfg(unix)]
fn restrict(path: &Path, mode: u32) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))
}

#[cfg(not(unix))]
fn restrict(_path: &Path, _mode: u32) -> io::Result<()> {
    Ok(())
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn mode(path: &Path) -> u32 {
        std::fs::metadata(path).unwrap().permissions().mode() & 0o777
    }

    #[test]
    fn folders_and_files_become_owner_only() {
        let base = std::env::temp_dir().join(format!("zeima-perm-{}", std::process::id()));
        let dir = base.join("data");
        private_dir(&dir).unwrap();
        assert_eq!(mode(&dir), 0o700);
        let file = dir.join("zeima.db");
        std::fs::write(&file, b"x").unwrap();
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o644)).unwrap();
        private_file(&file).unwrap();
        assert_eq!(mode(&file), 0o600);
        // A file that is not there is not an error.
        private_file(&dir.join("missing")).unwrap();
        // An existing, too open folder is tightened.
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).unwrap();
        private_dir(&dir).unwrap();
        assert_eq!(mode(&dir), 0o700);
        let _ = std::fs::remove_dir_all(&base);
    }
}
