//! Runtime workspace discovery without embedding the build checkout path.

use std::env;
use std::path::{Path, PathBuf};

const GHOST_WORKSPACE_ROOT_VAR: &str = "GHOST_WORKSPACE_ROOT";

pub(crate) fn detect_workspace_root() -> PathBuf {
    let current_dir = env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    detect_workspace_root_from(
        env::var_os(GHOST_WORKSPACE_ROOT_VAR).map(PathBuf::from),
        env::current_exe().ok(),
        current_dir,
    )
}

fn detect_workspace_root_from(
    explicit_root: Option<PathBuf>,
    current_exe: Option<PathBuf>,
    current_dir: PathBuf,
) -> PathBuf {
    if let Some(explicit_root) = explicit_root.filter(|path| !path.as_os_str().is_empty()) {
        return if explicit_root.is_absolute() {
            explicit_root
        } else {
            current_dir.join(explicit_root)
        };
    }

    if let Some(executable) = current_exe {
        for ancestor in executable.parent().into_iter().flat_map(Path::ancestors) {
            if ancestor.join("Cargo.toml").is_file()
                && ancestor.join("gui-backend").join("static").is_dir()
            {
                return ancestor.to_path_buf();
            }
        }
    }

    current_dir
}

#[cfg(test)]
mod tests {
    use super::detect_workspace_root_from;
    use std::fs;
    use std::path::PathBuf;

    #[test]
    fn explicit_relative_root_is_resolved_at_runtime() {
        assert_eq!(
            detect_workspace_root_from(
                Some(PathBuf::from("ghost")),
                None,
                PathBuf::from("/runtime"),
            ),
            PathBuf::from("/runtime/ghost"),
        );
    }

    #[test]
    fn release_binary_discovers_workspace_without_compile_time_path() {
        let root = std::env::temp_dir().join(format!(
            "ghost-gui-workspace-{}-{}",
            std::process::id(),
            std::thread::current().name().unwrap_or("unnamed")
        ));
        let static_dir = root.join("gui-backend/static");
        fs::create_dir_all(&static_dir).expect("create test workspace");
        fs::write(root.join("Cargo.toml"), "[workspace]\n").expect("write Cargo.toml");

        let resolved = detect_workspace_root_from(
            None,
            Some(root.join("target/release/ghost-launcher")),
            PathBuf::from("/fallback"),
        );

        assert_eq!(resolved, root);
        fs::remove_dir_all(&resolved).expect("remove test workspace");
    }
}
