//! Script semantics layered on top of Herdr-owned tabs/panes.
//!
//! [INPUT]: Depends on HerdrClient (pane management/process info/send_text), gpui-component (Input/Select/Dialog controls), ui::controls (Toggle switch), ProjectIndex and ScriptRegistry persistence
//! [OUTPUT]: Exposes ScriptRecord, ScriptDefinition, ScriptRegistry, open_new_script_dialog, active_project_scripts, run_script_id, and other script orchestration capabilities
//! [POS]: Shardlane script and background-service abstraction layer; provides Script/Service state and interaction panels to the Sidebar/Header

use super::*;
use crate::ui::controls::{ControlSurface, Toggle};
use crate::ui_metrics::DIALOG_CONTENT_GAP;
use crate::workspace_model::{build_project_index, project_paths_match, ProjectIndex, ProjectKey};
use gpui_component::{
    dialog::DialogButtonProps,
    input::{Input, InputState},
    select::{SearchableVec, Select, SelectState},
};
use serde::{Deserialize, Serialize};
use std::cell::Cell;
use std::collections::{BTreeSet, HashMap};
use std::ops::{Deref, DerefMut};
use std::process::Command;
use std::rc::Rc;
use std::time::{SystemTime, UNIX_EPOCH};

mod control;
mod discovery;
mod editor;
mod launch;
mod model;
mod monitor;
mod observe;
mod ports;

pub(crate) use model::{
    script_icon_name, ObservedService, ScriptDefinition, ScriptKind, ScriptRecord, ScriptRegistry,
    ScriptRuntimeProjection, ScriptStatus,
};
