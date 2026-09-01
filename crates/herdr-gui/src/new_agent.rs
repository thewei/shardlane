//! New Agent composer and Herdr-owned launch orchestration.
//!
//! Codex-style agent launch composer: branded Agent picker, Plan/Build segmented modes,
//! removable attachments, empty-Project onboarding state. Shardlane owns only the preflight
//! UI and the launch intent; agent processes, PTYs, Tabs, Panes, focus, and terminal input
//! remain owned by Herdr.
//! ChatGPT-like layout: a sentence title (What should we do in [Project]? with a dashed
//! inline picker) vertically centered in the remaining space, with the composer card sunk
//! to the bottom (720 wide r13: py10 card + row-level px10 + attachment row pinned on
//! top, plus a 14px bare field with no prefix + mt8 control row + inverted 26×26
//! circular send button + h28 branch context row under the card); the control row has
//! agent/mode▾ on the left and attach/send on the right.
//!
//! [INPUT]: Depends on main.rs's ShardlaneApp/ProjectIndex/state/scripts application surface,
//! and on gpui-component's Input/SelectState/DropdownButton/PopupMenuItem.
//! [OUTPUT]: Exposes the new_agent_page render entry and open/attach/submit coordination methods (pub(super)),
//! plus @ file / slash-command completion (three layers: reference/reference_index/reference_provider:
//! pure functions → read-only catalog scanning → gpui-component CompletionMenu mounting).
//! [POS]: herdr-gui's New Agent landing page, consumed via main.rs's secondary surface dispatch.

use super::*;
use crate::agent_cli::resolve_agent_launch;
use crate::assets::agent_brand_icon;
use crate::composer_chip::ComposerChip;
use crate::scripts::ScriptRecord;
use crate::workspace_model::{
    sidebar_project_path_for_context, visible_sidebar_project_by_runtime_id, VisibleSidebarProject,
};
use ::gpui::img;
use ::gpui::Point;
use gpui_component::{
    input::{Input, InputEvent, InputState},
    menu::DropdownMenu as _,
    select::{SearchableVec, SelectEvent, SelectItem, SelectState},
};
use std::path::PathBuf;
use std::process::Command;
use std::time::{Duration, Instant};

mod branch;
mod launch;
mod model;
mod page;
mod reference;
pub(crate) mod reference_index;
pub(crate) mod reference_provider;
mod surface;

use branch::*;
use launch::*;
use model::*;

pub(crate) use model::{
    NewAgentMode, NewAgentPermission, NewAgentProjectChoice, NewAgentUiState, NewTabKind,
};
