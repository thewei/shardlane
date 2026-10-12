package history

/**
 * [INPUT]: 依赖用户数据目录路径与旧版本遗留的 history.sqlite3 家族文件
 * [OUTPUT]: 提供 RemoveLegacyCatalog：一次性删除旧 SQLite 目录（含 WAL/SHM/journal）并返回释放的字节数
 * [POS]: 迁移清理：旧版把会话正文复制进 FTS/页缓存；现在永远现读原始文件，这些副本不再有任何读者
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */

import (
	"errors"
	"os"
	"path/filepath"
)

// LegacyCatalogFileName is the SQLite catalog earlier builds kept inside the
// user-data directory (session metadata, an FTS copy of every message and a
// transcript page cache).
const LegacyCatalogFileName = "history.sqlite3"

// RemoveLegacyCatalog deletes the legacy catalog and its SQLite sidecar files
// from dir. Absent files are fine, so it is safe to call on every launch; the
// freed byte count covers the files actually removed.
func RemoveLegacyCatalog(dir string) (int64, error) {
	var freed int64
	var failures error
	for _, suffix := range []string{"", "-wal", "-shm", "-journal"} {
		path := filepath.Join(dir, LegacyCatalogFileName+suffix)
		info, err := os.Stat(path)
		if err != nil {
			continue
		}
		if err := os.Remove(path); err != nil {
			failures = errors.Join(failures, err)
			continue
		}
		freed += info.Size()
	}
	return freed, failures
}
