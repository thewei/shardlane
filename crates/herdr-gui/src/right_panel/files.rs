//! [INPUT]: std::fs, std::path, gpui, gpui_component, theme
//! [OUTPUT]: Provides WorkingTreeEntry, collect_working_tree, file_icon_for_path, read_file_content
//! [POS]: The right-panel file browsing and viewing module of crates/herdr-gui

use std::collections::HashSet;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorkingTreeEntry {
    pub relative_path: String,
    pub absolute_path: PathBuf,
    pub name: String,
    pub is_dir: bool,
    pub file_icon: &'static str,
    pub expanded: bool,
    pub depth: usize,
}

pub fn file_icon_for_name(name: &str) -> &'static str {
    let name_lower = name.to_ascii_lowercase();
    let named_icon = if name_lower.starts_with("readme") {
        Some("icons/file-types/readme.svg")
    } else if name_lower.starts_with("license")
        || name_lower.starts_with("licence")
        || name_lower.starts_with("copying")
    {
        Some("icons/file-types/certificate.svg")
    } else if name_lower.starts_with("dockerfile") || name_lower.starts_with("compose.") {
        Some("icons/file-types/docker.svg")
    } else if name_lower == "cmakelists.txt" || name_lower.starts_with("cmake.") {
        Some("icons/file-types/cmake.svg")
    } else if name_lower == "makefile"
        || name_lower.starts_with("makefile.")
        || name_lower == "justfile"
    {
        Some("icons/file-types/makefile.svg")
    } else if matches!(
        name_lower.as_str(),
        "cargo.toml" | "cargo.lock" | "rust-toolchain.toml"
    ) {
        Some("icons/file-types/rust.svg")
    } else if matches!(name_lower.as_str(), "go.mod" | "go.sum" | "go.work") {
        Some("icons/file-types/go.svg")
    } else if name_lower == "pyproject.toml"
        || name_lower == "pipfile"
        || name_lower.starts_with("requirements")
    {
        Some("icons/file-types/python.svg")
    } else if matches!(
        name_lower.as_str(),
        "bun.lock" | "bun.lockb" | "bunfig.toml"
    ) {
        Some("icons/file-types/bun.svg")
    } else if name_lower.starts_with("pnpm-") || name_lower == ".pnpmfile.cjs" {
        Some("icons/file-types/pnpm.svg")
    } else if name_lower == "yarn.lock" || name_lower.starts_with(".yarnrc") {
        Some("icons/file-types/yarn.svg")
    } else if name_lower == "package.json" {
        Some("icons/file-types/nodejs.svg")
    } else if name_lower == "package-lock.json" {
        Some("icons/file-types/npm.svg")
    } else if name_lower.starts_with("tsconfig.") || name_lower == "tsconfig.json" {
        Some("icons/file-types/typescript.svg")
    } else if name_lower.starts_with("jsconfig.") || name_lower == "jsconfig.json" {
        Some("icons/file-types/javascript.svg")
    } else if name_lower == ".gitignore"
        || name_lower == ".gitattributes"
        || name_lower == ".gitmodules"
        || name_lower == ".gitconfig"
    {
        Some("icons/file-types/git.svg")
    } else if name_lower == ".editorconfig" {
        Some("icons/file-types/editorconfig.svg")
    } else if name_lower.starts_with(".env") {
        Some("icons/file-types/settings.svg")
    } else if name_lower.starts_with(".prettier") || name_lower.starts_with("prettier.config.") {
        Some("icons/file-types/prettier.svg")
    } else if name_lower.starts_with(".eslint") || name_lower.starts_with("eslint.config.") {
        Some("icons/file-types/eslint.svg")
    } else if name_lower.starts_with("biome.json") {
        Some("icons/file-types/biome.svg")
    } else if name_lower.starts_with("vite.config.") {
        Some("icons/file-types/vite.svg")
    } else if name_lower.starts_with("tailwind.config.") {
        Some("icons/file-types/tailwindcss.svg")
    } else if name_lower.starts_with("svelte.config.") {
        Some("icons/file-types/svelte.svg")
    } else if name_lower.starts_with("vue.config.") {
        Some("icons/file-types/vue.svg")
    } else {
        None
    };

    if let Some(icon) = named_icon {
        return icon;
    }

    let extension = Path::new(name)
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or("");
    match extension {
        "rs" => "icons/file-types/rust.svg",
        "js" | "mjs" | "cjs" => "icons/file-types/javascript.svg",
        "ts" | "mts" | "cts" => "icons/file-types/typescript.svg",
        "jsx" | "tsx" => "icons/file-types/react.svg",
        "py" | "pyi" | "pyw" => "icons/file-types/python.svg",
        "go" => "icons/file-types/go.svg",
        "c" | "h" => "icons/file-types/c.svg",
        "cc" | "cpp" | "cxx" | "hh" | "hpp" => "icons/file-types/cpp.svg",
        "cs" => "icons/file-types/csharp.svg",
        "swift" => "icons/file-types/swift.svg",
        "kt" | "kts" => "icons/file-types/kotlin.svg",
        "java" => "icons/file-types/java.svg",
        "rb" => "icons/file-types/ruby.svg",
        "php" => "icons/file-types/php.svg",
        "html" | "htm" => "icons/file-types/html.svg",
        "css" | "less" => "icons/file-types/css.svg",
        "scss" | "sass" => "icons/file-types/sass.svg",
        "json" | "jsonc" | "jsonl" => "icons/file-types/json.svg",
        "yaml" | "yml" => "icons/file-types/yaml.svg",
        "toml" | "ini" | "cfg" => "icons/file-types/settings.svg",
        "xml" | "plist" => "icons/file-types/xml.svg",
        "md" | "mdx" | "markdown" => "icons/file-types/markdown.svg",
        "sh" | "bash" | "zsh" => "icons/file-types/console.svg",
        "sql" | "db" | "sqlite" => "icons/file-types/database.svg",
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "svg" | "ico" => "icons/file-types/image.svg",
        "zip" | "tar" | "gz" => "icons/file-types/zip.svg",
        "vue" => "icons/file-types/vue.svg",
        "svelte" => "icons/file-types/svelte.svg",
        "zig" => "icons/file-types/zig.svg",
        "lua" => "icons/file-types/lua.svg",
        _ => "icons/file.svg",
    }
}

pub fn file_icon_for_path(path: &str) -> &'static str {
    let name = Path::new(path)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(path);
    file_icon_for_name(name)
}

pub fn collect_working_tree(
    root: &Path,
    expanded_paths: &HashSet<PathBuf>,
) -> Vec<WorkingTreeEntry> {
    fn visit(
        dir: &Path,
        rel_dir: &Path,
        depth: usize,
        expanded_paths: &HashSet<PathBuf>,
        entries: &mut Vec<WorkingTreeEntry>,
    ) {
        if depth > 10 {
            return;
        }
        let Ok(read_dir) = std::fs::read_dir(dir) else {
            return;
        };
        let mut children = read_dir
            .filter_map(Result::ok)
            .filter_map(|entry| {
                let name = entry.file_name().to_string_lossy().into_owned();
                if name == ".git"
                    || name == "target"
                    || name == "node_modules"
                    || name == ".DS_Store"
                {
                    return None;
                }
                let is_dir = entry.file_type().ok()?.is_dir();
                Some((entry.path(), name, is_dir))
            })
            .collect::<Vec<_>>();

        children.sort_by(|(_a_path, a_name, a_is_dir), (_b_path, b_name, b_is_dir)| {
            if *a_is_dir != *b_is_dir {
                b_is_dir.cmp(a_is_dir)
            } else {
                a_name.to_lowercase().cmp(&b_name.to_lowercase())
            }
        });

        for (absolute_path, name, is_dir) in children {
            let relative_path = if rel_dir.as_os_str().is_empty() {
                name.clone()
            } else {
                rel_dir.join(&name).to_string_lossy().into_owned()
            };
            let expanded = is_dir && expanded_paths.contains(&absolute_path);
            let file_icon = file_icon_for_name(&name);

            entries.push(WorkingTreeEntry {
                relative_path: relative_path.clone(),
                absolute_path: absolute_path.clone(),
                name: name.clone(),
                is_dir,
                file_icon,
                expanded,
                depth,
            });

            if expanded {
                visit(
                    &absolute_path,
                    &rel_dir.join(&name),
                    depth + 1,
                    expanded_paths,
                    entries,
                );
            }
        }
    }

    let mut entries = Vec::new();
    visit(root, Path::new(""), 0, expanded_paths, &mut entries);
    entries
}

pub fn read_file_content(root: &Path, relative_path: &str) -> Result<String, String> {
    let full_path = root.join(relative_path);
    if !full_path.exists() {
        return Err("File does not exist".to_string());
    }
    match std::fs::read_to_string(&full_path) {
        Ok(content) => Ok(content),
        Err(err) => Err(format!("Unable to read file: {err}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_icon_mapping_recognizes_special_files_and_extensions() {
        assert_eq!(
            file_icon_for_name("Cargo.toml"),
            "icons/file-types/rust.svg"
        );
        assert_eq!(
            file_icon_for_name("package.json"),
            "icons/file-types/nodejs.svg"
        );
        assert_eq!(
            file_icon_for_name("tsconfig.json"),
            "icons/file-types/typescript.svg"
        );
        assert_eq!(
            file_icon_for_name("README.md"),
            "icons/file-types/readme.svg"
        );
        assert_eq!(file_icon_for_name("main.rs"), "icons/file-types/rust.svg");
        assert_eq!(file_icon_for_name("app.tsx"), "icons/file-types/react.svg");
        assert_eq!(
            file_icon_for_name("script.py"),
            "icons/file-types/python.svg"
        );
        assert_eq!(file_icon_for_name("styles.css"), "icons/file-types/css.svg");
        assert_eq!(file_icon_for_name("unknown.xyz"), "icons/file.svg");
    }

    #[test]
    fn file_icon_for_path_resolves_basename() {
        assert_eq!(
            file_icon_for_path("src/components/Header.tsx"),
            "icons/file-types/react.svg"
        );
        assert_eq!(
            file_icon_for_path("crates/herdr-gui/Cargo.toml"),
            "icons/file-types/rust.svg"
        );
    }
}
