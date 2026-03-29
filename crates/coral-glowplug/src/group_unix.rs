// SPDX-License-Identifier: AGPL-3.0-only
//! Unix group database path and GID resolution for JSON-RPC socket ownership.
//!
//! Override the passwd-style group file with `CORALREEF_GROUP_FILE` (default `/etc/group`).

/// Default group file when `CORALREEF_GROUP_FILE` is unset.
pub const DEFAULT_GROUP_FILE: &str = "/etc/group";

/// Path to the group database file (`CORALREEF_GROUP_FILE` or [`DEFAULT_GROUP_FILE`]).
#[must_use]
pub fn group_database_path() -> String {
    std::env::var("CORALREEF_GROUP_FILE")
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| DEFAULT_GROUP_FILE.to_string())
}

/// Resolves the numeric GID for `group_name` from the configured group file.
#[must_use]
pub fn gid_for_group_name(group_name: &str) -> Option<u32> {
    let content = std::fs::read_to_string(group_database_path()).ok()?;
    gid_from_group_file_content(&content, group_name)
}

fn gid_from_group_file_content(content: &str, group_name: &str) -> Option<u32> {
    for line in content.lines() {
        let fields: Vec<&str> = line.split(':').collect();
        if fields.len() >= 3 && fields[0] == group_name {
            return fields[2].parse().ok();
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gid_from_group_file_content_parses_line() {
        assert_eq!(
            gid_from_group_file_content("coralreef_grp:x:424242:\n", "coralreef_grp"),
            Some(424_242)
        );
    }

    #[test]
    fn gid_from_group_file_content_unknown_returns_none() {
        assert_eq!(gid_from_group_file_content("other:x:1:\n", "missing"), None);
    }
}
