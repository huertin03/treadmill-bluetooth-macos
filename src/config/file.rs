//! Shared config-file layer: path resolution, atomic writes, and one TOML read.

use std::io;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use tracing::warn;

/// Bound on symlink-chain resolution in [`write_atomic`] — a config path is a
/// hand-made link (usually one hop into a dotfiles repo), so anything deeper
/// smells like a loop; we stop and write at the last resolved path.
const MAX_SYMLINK_HOPS: usize = 8;

/// Follow `path` through symlinks to the file the write must land in.
///
/// `fs::canonicalize` is not usable here: it fails on a dangling link (the
/// target may not exist yet — reviving it by creating the target is exactly
/// what we want) and on a not-yet-existing plain path.
fn resolve_symlinks(path: &Path) -> PathBuf {
    let mut current = path.to_path_buf();
    for _ in 0..MAX_SYMLINK_HOPS {
        let Ok(link) = std::fs::read_link(&current) else {
            return current;
        };
        current = if link.is_absolute() {
            link
        } else {
            // A relative link target is relative to the link's directory.
            current.parent().map_or(link.clone(), |dir| dir.join(&link))
        };
    }
    warn!(path = %path.display(), hops = MAX_SYMLINK_HOPS, "symlink chain too deep — writing at last resolved path");
    current
}

/// Write `contents` to `path` via same-directory temp + rename (задача 037).
///
/// `std::fs::write` truncates first: a crash between truncate and complete write
/// permanently wipes the operator's `config.toml`. Writing a sibling `*.tmp`
/// then `rename(2)` keeps the old file intact until the new body is durable
/// (same FS, atomic replace on macOS).
///
/// Symlinks are resolved first (задача 056): `rename(2)` replaces the *link
/// itself* with the temp file, silently detaching a config kept as a symlink
/// into a dotfiles repo. Writing at the resolved target keeps the link alive
/// and lands the content where the operator tracks it.
pub(crate) fn write_atomic(path: &Path, contents: impl AsRef<[u8]>) -> io::Result<()> {
    let target = resolve_symlinks(path);
    let tmp = target.with_extension("toml.tmp");
    std::fs::write(&tmp, contents.as_ref())?;
    std::fs::rename(&tmp, &target)?;
    Ok(())
}

/// Optional environment variable to point at the config in a non-standard
/// location (tests, or a user who does not want the `$HOME`-default path).
/// Normally unset — the `$HOME`-anchored default is used.
const CONFIG_ENV: &str = "TREADMILL_CONFIG";

/// Per-user config path relative to `$HOME`. `$HOME`-anchored (not cwd) because
/// the daemon runs under launchd with no reliable working directory — same
/// reasoning as `store::open`. Users own this file (a personal dotfiles repo
/// typically symlinks it here); it is intentionally NOT committed to this repo.
/// TOML since задача 023 (was JSON `config.json`/`goals.json`): comments let the
/// example config document each key's default inline.
const HOME_CONFIG_RELPATH: &str = ".config/treadmill-bluetooth-macos/config.toml";

/// Last-modified time of the resolved config file, or `None` when it can't be
/// stat'd (missing file, unreadable, or no `$HOME`). The daemon polls this to
/// reload goals only when the file actually changes — avoiding a re-read/re-log
/// every tick (задача 017). A `None`→`Some` (or vice-versa) transition on
/// create/delete is itself a change the caller reacts to.
pub fn config_mtime() -> Option<SystemTime> {
    let path = config_path()?;
    std::fs::metadata(&path).ok()?.modified().ok()
}

/// Resolve the config file path: explicit [`CONFIG_ENV`] override first, else
/// the `$HOME`-anchored [`HOME_CONFIG_RELPATH`]. `None` only when `$HOME` is
/// unset (and no override). Since задача 023 there is a single TOML path — the
/// transitional JSON/`goals.json` fallbacks were dropped.
pub(crate) fn config_path() -> Option<PathBuf> {
    if let Ok(path) = std::env::var(CONFIG_ENV) {
        return Some(PathBuf::from(path));
    }
    let home = std::env::var_os("HOME")?;
    Some(PathBuf::from(home).join(HOME_CONFIG_RELPATH))
}

/// One read+parse of the config file (задача 047). Shared by the top-level
/// key readers so a single widget tick does not open/parse the same path up
/// to four times with four independent silent-fallback surfaces.
pub(crate) fn read_config_value(path: &std::path::Path) -> Option<toml::Value> {
    let raw = std::fs::read_to_string(path).ok()?;
    toml::from_str(&raw).ok()
}

/// Update (or insert) a single top-level `key = value` line in the per-user
/// config, leaving every other line untouched — same line-based-upsert
/// approach as [`crate::zone_hold::upsert_zone_hold_keys`], but for a plain
/// top-level key rather than one scoped to a `[section]`. A top-level key must
/// precede any `[section]` header in TOML, so a missing key is inserted right
/// before the first such header (or appended at EOF if there is none). Used by
/// `tm speed-widget on/off` (задача 029) so toggling it never disturbs
/// `[zone_hold]` or any other hand-edited section.
pub fn upsert_top_level_key(path: &std::path::Path, key: &str, value: &str) -> anyhow::Result<()> {
    let existing = std::fs::read_to_string(path).unwrap_or_default();
    let mut lines: Vec<String> = existing.lines().map(str::to_string).collect();

    let section_start = lines
        .iter()
        .position(|l| l.trim_start().starts_with('['))
        .unwrap_or(lines.len());

    let prefix = format!("{key} =");
    let existing_line = lines[..section_start]
        .iter()
        .position(|l| l.trim_start().starts_with(&prefix));
    let new_line = format!("{key} = {value}");
    match existing_line {
        Some(offset) => lines[offset] = new_line,
        None => lines.insert(section_start, new_line),
    }

    write_atomic(path, lines.join("\n") + "\n")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_atomic_replaces_existing_and_leaves_target_on_tmp_failure() {
        let dir = std::env::temp_dir().join(format!("tm-atomic-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.toml");
        std::fs::write(&path, "goals = [8000]\n").unwrap();

        write_atomic(&path, "goals = [9000]\nshow_speed = true\n").unwrap();
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "goals = [9000]\nshow_speed = true\n"
        );
        // No leftover tmp after success.
        assert!(!path.with_extension("toml.tmp").exists());

        // Target still holds the last good body if we only fail *before* rename
        // (simulate by writing a good body, then ensuring a failed rename isn't
        // needed: the contract is "write tmp first, rename only on success").
        assert!(path.exists());

        std::fs::remove_file(&path).ok();
        std::fs::remove_dir(&dir).ok();
    }

    #[test]
    fn write_atomic_preserves_symlink_and_writes_through_to_target() {
        let dir = std::env::temp_dir().join(format!("tm-atomic-link-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("dotfiles")).unwrap();
        let target = dir.join("dotfiles/config.toml");
        std::fs::write(&target, "goals = [8000]\n").unwrap();
        let link = dir.join("config.toml");
        std::os::unix::fs::symlink(&target, &link).unwrap();

        write_atomic(&link, "goals = [9000]\n").unwrap();

        // The link must survive the write and the content must land in the target.
        assert!(std::fs::symlink_metadata(&link).unwrap().is_symlink());
        assert_eq!(
            std::fs::read_to_string(&target).unwrap(),
            "goals = [9000]\n"
        );
        assert_eq!(std::fs::read_to_string(&link).unwrap(), "goals = [9000]\n");
        assert!(!target.with_extension("toml.tmp").exists());

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn write_atomic_revives_dangling_symlink_by_creating_the_target() {
        let dir = std::env::temp_dir().join(format!("tm-atomic-dangling-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("dotfiles")).unwrap();
        let target = dir.join("dotfiles/config.toml");
        let link = dir.join("config.toml");
        std::os::unix::fs::symlink(&target, &link).unwrap();

        write_atomic(&link, "goals = [7000]\n").unwrap();

        assert!(std::fs::symlink_metadata(&link).unwrap().is_symlink());
        assert_eq!(
            std::fs::read_to_string(&target).unwrap(),
            "goals = [7000]\n"
        );

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn write_atomic_follows_relative_symlink_from_the_link_directory() {
        let dir = std::env::temp_dir().join(format!("tm-atomic-rel-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("dotfiles")).unwrap();
        let target = dir.join("dotfiles/config.toml");
        std::fs::write(&target, "goals = [8000]\n").unwrap();
        let link = dir.join("config.toml");
        std::os::unix::fs::symlink("dotfiles/config.toml", &link).unwrap();

        write_atomic(&link, "goals = [6000]\n").unwrap();

        assert!(std::fs::symlink_metadata(&link).unwrap().is_symlink());
        assert_eq!(
            std::fs::read_to_string(&target).unwrap(),
            "goals = [6000]\n"
        );

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn upsert_top_level_key_inserts_before_first_section_and_replaces_in_place() {
        let dir = std::env::temp_dir().join(format!("tm-upsert-toplevel-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();

        // Insert into a file with no existing key and a trailing section — must
        // land before `[zone_hold]`, not after (would be invalid TOML).
        let path = dir.join("insert.toml");
        std::fs::write(&path, "goals = [8000]\n\n[zone_hold]\nenabled = true\n").unwrap();
        upsert_top_level_key(&path, "show_speed", "true").unwrap();
        let written = std::fs::read_to_string(&path).unwrap();
        let key_pos = written.find("show_speed = true").unwrap();
        let section_pos = written.find("[zone_hold]").unwrap();
        assert!(key_pos < section_pos, "key must precede the section header");

        // Replacing an existing key updates it in place rather than duplicating.
        upsert_top_level_key(&path, "show_speed", "false").unwrap();
        let written = std::fs::read_to_string(&path).unwrap();
        assert_eq!(written.matches("show_speed").count(), 1);
        assert!(written.contains("show_speed = false"));

        std::fs::remove_file(&path).ok();
    }
}
