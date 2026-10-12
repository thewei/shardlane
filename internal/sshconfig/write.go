package sshconfig

/**
 * [INPUT]: 用户显式保存动作传入的配置文件路径与完整文本
 * [OUTPUT]: SaveFile —— 无损写回：跟随符号链接写真实目标、保留文件权限、先落 .bak 一代备份再原子替换
 * [POS]: 无损编辑的安全边界：备份失败即中止（绝不无备份覆盖）；不记录文件内容，只返回错误
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */

import (
	"fmt"
	"io/fs"
	"os"
	"path/filepath"
)

// SaveFile replaces path's content with exactly the given text: no
// normalization, no re-serialization, so comments and unknown directives
// survive by construction. It writes the symlink's target (a linked config
// keeps pointing at its real file), keeps one previous copy next to the
// target as "<name>.bak", and preserves the file mode (0600 for a fresh
// config).
func SaveFile(path, content string) error {
	target := path
	if fi, err := os.Lstat(path); err == nil && fi.Mode()&os.ModeSymlink != 0 {
		resolved, err := filepath.EvalSymlinks(path)
		if err != nil {
			return fmt.Errorf("resolve symlink %s: %w", path, err)
		}
		target = resolved
	}

	mode := fs.FileMode(0o600)
	if fi, err := os.Stat(target); err == nil {
		mode = fi.Mode().Perm()
		previous, err := os.ReadFile(target)
		if err != nil {
			return fmt.Errorf("back up %s: %w", target, err)
		}
		if err := os.WriteFile(target+".bak", previous, mode); err != nil {
			return fmt.Errorf("write backup %s.bak: %w", target, err)
		}
	} else if err := os.MkdirAll(filepath.Dir(target), 0o700); err != nil {
		return fmt.Errorf("create %s: %w", filepath.Dir(target), err)
	}

	// Stage in the target's directory so the final rename stays on one
	// filesystem, then flip it in place.
	tmp, err := os.CreateTemp(filepath.Dir(target), ".shardlane-ssh-config-*")
	if err != nil {
		return fmt.Errorf("stage temp file: %w", err)
	}
	tmpName := tmp.Name()
	defer os.Remove(tmpName) // no-op once the rename succeeded
	if _, err := tmp.WriteString(content); err != nil {
		tmp.Close()
		return fmt.Errorf("write temp file: %w", err)
	}
	if err := tmp.Chmod(mode); err != nil {
		tmp.Close()
		return fmt.Errorf("set temp file mode: %w", err)
	}
	if err := tmp.Close(); err != nil {
		return fmt.Errorf("close temp file: %w", err)
	}
	if err := os.Rename(tmpName, target); err != nil {
		return fmt.Errorf("replace %s: %w", target, err)
	}
	return nil
}
