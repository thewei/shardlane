//! [INPUT]: The import surface and types of the right_panel module root (`use super::*`).
//! [OUTPUT]: render_right_panel_files — the file-tree browsing view; clicking a file opens the
//! full-content preview surface (ShardlaneApp::open_file_preview), context menu and toolbar for
//! creating files/folders, renaming, deleting, and dragging external files in.
//! [POS]: The files_view responsibility slice of the right_panel directory.
use super::*;
use gpui_component::dialog::DialogButtonProps;
use std::fs;
use std::path::{Path, PathBuf};

impl ShardlaneApp {
    pub(super) fn render_right_panel_files(
        &self,
        theme: ContentSurfaceTheme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let herdr = cx.entity();
        let entries = self.right_panel.working_tree.clone();
        let active_project_dir = self.active_project_path_for_right_panel();

        let mut list = div().flex().flex_col().py(px(4.0));

        if entries.is_empty() {
            list = list.child(crate::ui::empty_state::empty_state(
                "No files found in project workspace",
                theme.muted,
            ));
        }

        for entry in entries {
            let rel = entry.relative_path.clone();
            let abs = entry.absolute_path.clone();
            let is_dir = entry.is_dir;
            let file_icon = entry.file_icon;
            let depth = entry.depth;
            let name = entry.name.clone();
            let expanded = entry.expanded;
            let click_herdr = herdr.clone();
            let menu_herdr = herdr.clone();
            let drop_herdr = herdr.clone();

            let target_dir = if is_dir {
                abs.clone()
            } else {
                abs.parent()
                    .map(Path::to_path_buf)
                    .unwrap_or_else(|| abs.clone())
            };

            let menu_abs = abs.clone();
            let menu_name = name.clone();
            let menu_target_dir = target_dir.clone();

            let row = div()
                .h(px(26.0))
                .mx(px(4.0))
                .pl(px(6.0 + depth as f32 * 14.0))
                .pr(px(6.0))
                .rounded(px(5.0))
                .flex()
                .items_center()
                .gap(SPACE_ICON)
                .cursor_pointer()
                .hover(|e| e.bg(theme.foreground.opacity(crate::theme::WASH_HOVER)))
                .child(if is_dir {
                    Icon::empty()
                        .path(if expanded {
                            "icons/chevron-down.svg"
                        } else {
                            "icons/chevron-right.svg"
                        })
                        .with_size(px(10.0))
                        .text_color(theme.muted)
                        .into_any_element()
                } else {
                    div().size(px(10.0)).flex_none().into_any_element()
                })
                .child(
                    Icon::empty()
                        .path(if is_dir {
                            "icons/folder.svg"
                        } else {
                            file_icon
                        })
                        .with_size(px(14.0))
                        .text_color(theme.foreground),
                )
                .child(
                    div()
                        .min_w_0()
                        .flex_1()
                        .truncate()
                        .text_size(crate::theme::FONT_BODY)
                        .text_color(theme.foreground)
                        .child(name),
                )
                .on_mouse_down(MouseButton::Left, move |_, _, app| {
                    let abs_clone = abs.clone();
                    let rel_clone = rel.clone();
                    click_herdr.update(app, |this, cx| {
                        if is_dir {
                            if !this.right_panel.files_expanded_paths.remove(&abs_clone) {
                                this.right_panel.files_expanded_paths.insert(abs_clone);
                            }
                            this.refresh_right_panel_working_tree(cx);
                            this.persist_current_workspace_state();
                        } else {
                            // 2026-09-03: file preview moved to the full-content
                            // surface (covers the hosted TUI, keeps it alive).
                            this.open_file_preview(rel_clone, cx);
                        }
                    });
                })
                .when(is_dir, |row| {
                    let drop_dest = target_dir.clone();
                    let drop_herdr = drop_herdr.clone();
                    row.can_drop(|value, _, _| {
                        value.downcast_ref::<gpui::ExternalPaths>().is_some()
                    })
                    .drag_over::<gpui::ExternalPaths>(move |style, _, _, _| {
                        style.bg(theme.foreground.opacity(0.12))
                    })
                    .on_drop(
                        move |paths: &gpui::ExternalPaths, window, app| {
                            let dest = drop_dest.clone();
                            let src_paths = paths.paths().to_vec();
                            drop_herdr.update(app, |this, cx| {
                                this.copy_external_paths_to(src_paths, &dest, window, cx);
                            });
                        },
                    )
                })
                .context_menu(move |menu, _, _| {
                    let h_new_file = menu_herdr.clone();
                    let h_new_folder = menu_herdr.clone();
                    let h_rename = menu_herdr.clone();
                    let h_delete = menu_herdr.clone();
                    let dir_for_new = menu_target_dir.clone();
                    let dir_for_new_folder = menu_target_dir.clone();
                    let rename_path = menu_abs.clone();
                    let rename_name = menu_name.clone();
                    let delete_path = menu_abs.clone();
                    let delete_name = menu_name.clone();

                    menu.item(
                        PopupMenuItem::new("New File…")
                            .icon(Icon::empty().path("icons/file.svg").with_size(px(13.0)))
                            .on_click(move |_, window, app| {
                                let dir = dir_for_new.clone();
                                h_new_file.update(app, |this, cx| {
                                    this.prompt_create_file(dir, window, cx);
                                });
                            }),
                    )
                    .item(
                        PopupMenuItem::new("New Folder…")
                            .icon(
                                Icon::empty()
                                    .path("icons/folder-new.svg")
                                    .with_size(px(13.0)),
                            )
                            .on_click(move |_, window, app| {
                                let dir = dir_for_new_folder.clone();
                                h_new_folder.update(app, |this, cx| {
                                    this.prompt_create_folder(dir, window, cx);
                                });
                            }),
                    )
                    .separator()
                    .item(
                        PopupMenuItem::new("Rename…")
                            .icon(Icon::empty().path("icons/pencil.svg").with_size(px(13.0)))
                            .on_click(move |_, window, app| {
                                let p = rename_path.clone();
                                let n = rename_name.clone();
                                h_rename.update(app, |this, cx| {
                                    this.prompt_rename_file_or_dir(p, n, window, cx);
                                });
                            }),
                    )
                    .separator()
                    .item(
                        PopupMenuItem::new("Delete…")
                            .icon(Icon::empty().path("icons/trash.svg").with_size(px(13.0)))
                            .on_click(move |_, window, app| {
                                let p = delete_path.clone();
                                let n = delete_name.clone();
                                h_delete.update(app, |this, cx| {
                                    this.confirm_delete_file_or_dir(p, n, window, cx);
                                });
                            }),
                    )
                });

            list = list.child(row);
        }

        // Toolbar at the top: New File, New Folder, Refresh
        let toolbar = {
            let new_file_herdr = herdr.clone();
            let new_folder_herdr = herdr.clone();
            let refresh_herdr = herdr.clone();
            let project_root_file = active_project_dir.clone();
            let project_root_folder = active_project_dir.clone();

            h_flex()
                .h(px(28.0))
                .px(px(8.0))
                .items_center()
                .justify_between()
                .border_b_1()
                .border_color(theme.border)
                .child(
                    div()
                        .text_size(crate::theme::FONT_META)
                        .text_color(theme.muted)
                        .child("Files"),
                )
                .child(
                    h_flex()
                        .gap_1()
                        .items_center()
                        .child(
                            Button::new("files-new-file")
                                .xsmall()
                                .ghost()
                                .icon(Icon::empty().path("icons/file.svg").with_size(px(12.0)))
                                .tooltip("New File")
                                .on_click(move |_, window, app| {
                                    if let Some(root) = project_root_file.clone() {
                                        new_file_herdr.update(app, |this, cx| {
                                            this.prompt_create_file(root, window, cx);
                                        });
                                    }
                                }),
                        )
                        .child(
                            Button::new("files-new-folder")
                                .xsmall()
                                .ghost()
                                .icon(
                                    Icon::empty()
                                        .path("icons/folder-new.svg")
                                        .with_size(px(12.0)),
                                )
                                .tooltip("New Folder")
                                .on_click(move |_, window, app| {
                                    if let Some(root) = project_root_folder.clone() {
                                        new_folder_herdr.update(app, |this, cx| {
                                            this.prompt_create_folder(root, window, cx);
                                        });
                                    }
                                }),
                        )
                        .child(
                            Button::new("files-refresh")
                                .xsmall()
                                .ghost()
                                .icon(
                                    Icon::empty()
                                        .path("icons/refresh-cw.svg")
                                        .with_size(px(12.0)),
                                )
                                .tooltip("Refresh")
                                .on_click(move |_, _, app| {
                                    refresh_herdr.update(app, |this, cx| {
                                        this.refresh_right_panel_working_tree(cx);
                                    });
                                }),
                        ),
                )
        };

        // Root container with drop support for dropping files to project root
        let root_drop_dest = active_project_dir.clone();
        let root_drop_herdr = herdr.clone();

        let mut tree_view = div()
            .id("right-panel-files-tree")
            .size_full()
            .flex()
            .flex_col()
            .child(toolbar)
            .child(
                div()
                    .id("files-list-scroll")
                    .flex_1()
                    .overflow_y_scroll()
                    .child(list),
            );

        if let Some(dest) = root_drop_dest {
            tree_view = tree_view
                .can_drop(|value, _, _| value.downcast_ref::<gpui::ExternalPaths>().is_some())
                .drag_over::<gpui::ExternalPaths>(move |style, _, _, _| {
                    style.border_2().border_color(theme.foreground.opacity(0.4))
                })
                .on_drop(move |paths: &gpui::ExternalPaths, window, app| {
                    let d = dest.clone();
                    let src_paths = paths.paths().to_vec();
                    root_drop_herdr.update(app, |this, cx| {
                        this.copy_external_paths_to(src_paths, &d, window, cx);
                    });
                });
        }

        tree_view.into_any_element()
    }

    pub(crate) fn prompt_create_file(
        &mut self,
        dir: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if window.has_active_dialog(cx) {
            return;
        }
        let input = cx.new(|cx| InputState::new(window, cx).placeholder("filename.ext"));
        input.update(cx, |state, input_cx| {
            state.focus(window, input_cx);
        });

        let herdr = cx.entity();
        let dialog_input = input.clone();
        let dialog_width =
            responsive_dialog_width(window.bounds().size.width.to_f64(), 0.84, 300.0, 420.0);

        window.open_dialog(cx, move |dialog, _window, _cx| {
            let submit_herdr = herdr.clone();
            let submit_input = dialog_input.clone();
            let dest_dir = dir.clone();

            dialog
                .title("New File")
                .w(px(dialog_width))
                .button_props(
                    DialogButtonProps::default()
                        .ok_text("Create")
                        .cancel_text("Cancel"),
                )
                .footer(|ok, cancel, window, cx| vec![cancel(window, cx), ok(window, cx)])
                .child(Input::new(&dialog_input).w_full())
                .on_ok(move |_, window, app| {
                    let name = submit_input.read(app).value().trim().to_string();
                    if name.is_empty() {
                        return false;
                    }
                    let target_path = dest_dir.join(&name);
                    if target_path.exists() {
                        window.push_notification("A file with that name already exists", app);
                        return false;
                    }
                    if let Err(e) = fs::File::create(&target_path) {
                        window.push_notification(format!("Failed to create file: {e}"), app);
                        return false;
                    }
                    submit_herdr.update(app, |this, cx| {
                        this.refresh_right_panel_working_tree(cx);
                    });
                    true
                })
        });
    }

    pub(crate) fn prompt_create_folder(
        &mut self,
        dir: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if window.has_active_dialog(cx) {
            return;
        }
        let input = cx.new(|cx| InputState::new(window, cx).placeholder("folder_name"));
        input.update(cx, |state, input_cx| {
            state.focus(window, input_cx);
        });

        let herdr = cx.entity();
        let dialog_input = input.clone();
        let dialog_width =
            responsive_dialog_width(window.bounds().size.width.to_f64(), 0.84, 300.0, 420.0);

        window.open_dialog(cx, move |dialog, _window, _cx| {
            let submit_herdr = herdr.clone();
            let submit_input = dialog_input.clone();
            let dest_dir = dir.clone();

            dialog
                .title("New Folder")
                .w(px(dialog_width))
                .button_props(
                    DialogButtonProps::default()
                        .ok_text("Create")
                        .cancel_text("Cancel"),
                )
                .footer(|ok, cancel, window, cx| vec![cancel(window, cx), ok(window, cx)])
                .child(Input::new(&dialog_input).w_full())
                .on_ok(move |_, window, app| {
                    let name = submit_input.read(app).value().trim().to_string();
                    if name.is_empty() {
                        return false;
                    }
                    let target_path = dest_dir.join(&name);
                    if target_path.exists() {
                        window.push_notification("A folder with that name already exists", app);
                        return false;
                    }
                    if let Err(e) = fs::create_dir_all(&target_path) {
                        window.push_notification(format!("Failed to create folder: {e}"), app);
                        return false;
                    }
                    submit_herdr.update(app, |this, cx| {
                        this.right_panel.files_expanded_paths.insert(target_path);
                        this.refresh_right_panel_working_tree(cx);
                    });
                    true
                })
        });
    }

    pub(crate) fn prompt_rename_file_or_dir(
        &mut self,
        path: PathBuf,
        current_name: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if window.has_active_dialog(cx) {
            return;
        }
        let input = cx.new(|cx| InputState::new(window, cx).placeholder("New name"));
        let initial_name = current_name.clone();
        input.update(cx, |state, input_cx| {
            state.set_value(initial_name, window, input_cx);
            state.focus(window, input_cx);
        });

        let herdr = cx.entity();
        let dialog_input = input.clone();
        let dialog_width =
            responsive_dialog_width(window.bounds().size.width.to_f64(), 0.84, 300.0, 420.0);

        window.open_dialog(cx, move |dialog, _window, _cx| {
            let submit_herdr = herdr.clone();
            let submit_input = dialog_input.clone();
            let old_path = path.clone();

            dialog
                .title("Rename")
                .w(px(dialog_width))
                .button_props(
                    DialogButtonProps::default()
                        .ok_text("Rename")
                        .cancel_text("Cancel"),
                )
                .footer(|ok, cancel, window, cx| vec![cancel(window, cx), ok(window, cx)])
                .child(Input::new(&dialog_input).w_full())
                .on_ok(move |_, window, app| {
                    let new_name = submit_input.read(app).value().trim().to_string();
                    if new_name.is_empty() {
                        return false;
                    }
                    let Some(parent) = old_path.parent() else {
                        return false;
                    };
                    let new_path = parent.join(&new_name);
                    if new_path == old_path {
                        return true;
                    }
                    if new_path.exists() {
                        window.push_notification(
                            "A file or folder with that name already exists",
                            app,
                        );
                        return false;
                    }
                    if let Err(e) = fs::rename(&old_path, &new_path) {
                        window.push_notification(format!("Rename failed: {e}"), app);
                        return false;
                    }
                    submit_herdr.update(app, |this, cx| {
                        if this.right_panel.files_expanded_paths.remove(&old_path) {
                            this.right_panel.files_expanded_paths.insert(new_path);
                        }
                        this.refresh_right_panel_working_tree(cx);
                    });
                    true
                })
        });
    }

    pub(crate) fn confirm_delete_file_or_dir(
        &mut self,
        path: PathBuf,
        name: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if window.has_active_dialog(cx) {
            return;
        }
        let herdr = cx.entity();
        let is_dir = path.is_dir();
        let dialog_width =
            responsive_dialog_width(window.bounds().size.width.to_f64(), 0.84, 320.0, 440.0);

        window.open_dialog(cx, move |dialog, _window, cx| {
            let submit_herdr = herdr.clone();
            let target_path = path.clone();
            let item_type = if is_dir { "folder" } else { "file" };

            dialog
                .title(format!("Delete {name}"))
                .w(px(dialog_width))
                .button_props(
                    DialogButtonProps::default()
                        .ok_text("Delete")
                        .cancel_text("Cancel"),
                )
                .footer(|ok, cancel, window, cx| vec![cancel(window, cx), ok(window, cx)])
                .child(
                    v_flex()
                        .gap(crate::ui_metrics::DIALOG_CONTENT_GAP)
                        .child(div().text_size(crate::theme::FONT_BODY).child(format!(
                            "Are you sure you want to permanently delete this {item_type}?"
                        )))
                        .child(
                            div()
                                .text_size(crate::theme::FONT_META)
                                .text_color(cx.theme().danger)
                                .child("This will permanently remove it from your disk."),
                        )
                        .child(
                            div()
                                .text_size(crate::theme::FONT_META)
                                .text_color(cx.theme().muted_foreground)
                                .truncate()
                                .child(target_path.display().to_string()),
                        ),
                )
                .on_ok(move |_, window, app| {
                    let res = if target_path.is_dir() {
                        fs::remove_dir_all(&target_path)
                    } else {
                        fs::remove_file(&target_path)
                    };
                    if let Err(e) = res {
                        window.push_notification(format!("Delete failed: {e}"), app);
                        return false;
                    }
                    submit_herdr.update(app, |this, cx| {
                        this.right_panel.files_expanded_paths.remove(&target_path);
                        this.refresh_right_panel_working_tree(cx);
                    });
                    true
                })
        });
    }

    pub(crate) fn copy_external_paths_to(
        &mut self,
        sources: Vec<PathBuf>,
        destination_dir: &Path,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !destination_dir.is_dir() {
            return;
        }
        let mut count = 0usize;
        for src in sources {
            let Some(file_name) = src.file_name() else {
                continue;
            };
            let dest = destination_dir.join(file_name);
            let result = if src.is_dir() {
                copy_dir_recursive(&src, &dest)
            } else {
                fs::copy(&src, &dest).map(|_| ())
            };
            if result.is_ok() {
                count += 1;
            }
        }
        if count > 0 {
            window.push_notification(format!("Imported {count} items"), cx);
            self.refresh_right_panel_working_tree(cx);
        }
    }
}

fn copy_dir_recursive(src: &Path, dst: &Path) -> std::io::Result<()> {
    if !dst.exists() {
        fs::create_dir_all(dst)?;
    }
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        if src_path.is_dir() {
            copy_dir_recursive(&src_path, &dst_path)?;
        } else {
            fs::copy(&src_path, &dst_path)?;
        }
    }
    Ok(())
}
