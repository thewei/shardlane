package nativeui

/**
 * [INPUT]: 依赖 internal/herdr 的 ErrProtocolTooOld、MachineInstanceKey 与 Shell 失败分类
 * [OUTPUT]: 验证旧协议失败分类（本地可就地升级、远程/未知/其他失败不可）与更新流程的无窗口守卫
 * [POS]: runtime.go 编排层的分类与升级入口回归，配合 bootstrap_page_test 的呈现回归
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */

import (
	"errors"
	"fmt"
	"testing"

	"github.com/wh-studio/herdr-client/internal/herdr"
)

func TestUpgradeableFailureClassification(t *testing.T) {
	old := fmt.Errorf("open default: %w", herdr.ErrProtocolTooOld)
	if !upgradeableFailure("default", old) {
		t.Fatal("a local old-protocol server must be upgradeable in place")
	}
	if upgradeableFailure(herdr.MachineInstanceKey("abc"), old) {
		t.Fatal("remote machines upgrade on their own host, never from the client")
	}
	if upgradeableFailure("", old) {
		t.Fatal("an unknown instance must not arm the update affordance")
	}
	if upgradeableFailure("default", errors.New("herdr session list timed out")) {
		t.Fatal("unrelated failures must not arm the update affordance")
	}
}

func TestStartHerdrUpgradeRequiresWindow(t *testing.T) {
	s := NewShell()
	s.herdrUpgradeable = true
	s.win = nil
	s.startHerdrUpgrade()
	if s.herdrUpdating {
		t.Fatal("without a window there is nothing to update against")
	}
}
