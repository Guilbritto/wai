//! What the bars show: the keys the user hid or ordered, and the menu that toggles them.

use std::collections::BTreeSet;

use gpui_kit::component::menu::{PopupMenu, PopupMenuItem};
use gpui_kit::*;

use crate::sidebar::{SidebarTab, TABS};
use crate::workspace::Workspace;

/// `default`'s keys: those in `order` first, in its order, then the rest as `default` has them.
pub(crate) fn ordered(default: &[String], order: &[String]) -> Vec<String> {
    let first = order.iter().filter(|key| default.contains(key));
    let rest = default.iter().filter(|key| !order.contains(key));
    first.chain(rest).cloned().collect()
}

/// One hideable item in a bar's menu; entries with a group list under its heading.
pub(crate) struct HideEntry {
    pub key: String,
    pub label: SharedString,
    pub group: Option<SharedString>,
}

/// `menu` with a checked entry per item; a click shows or hides it.
pub(crate) fn hide_menu(
    mut menu: PopupMenu,
    entries: &[HideEntry],
    hidden: &BTreeSet<String>,
    workspace: WeakEntity<Workspace>,
) -> PopupMenu {
    let mut groups: Vec<&SharedString> = Vec::new();
    for group in entries.iter().filter_map(|e| e.group.as_ref()) {
        if !groups.contains(&group) {
            groups.push(group);
        }
    }
    let section = |menu: PopupMenu, group: Option<&SharedString>| {
        entries
            .iter()
            .filter(|e| e.group.as_ref() == group)
            .fold(menu, |menu, e| {
                let (key, workspace) = (e.key.clone(), workspace.clone());
                menu.item(
                    PopupMenuItem::new(e.label.clone())
                        .checked(!hidden.contains(&e.key))
                        .on_click(move |_, _, cx| {
                            workspace.update(cx, |w, cx| w.toggle_hidden(&key, cx)).ok();
                        }),
                )
            })
    };
    menu = section(menu, None);
    for group in groups {
        menu = menu.item(PopupMenuItem::label(group.clone()));
        menu = section(menu, Some(group));
    }
    menu
}

impl Workspace {
    pub(crate) fn shown(&self, key: &str) -> bool {
        !self.hidden.contains(key)
    }

    /// Hiding the selected sidebar tab shows the sessions; a main panel's pane stays open.
    pub(crate) fn toggle_hidden(&mut self, key: &str, cx: &mut Context<Self>) {
        if !self.hidden.remove(key) {
            self.hidden.insert(key.to_string());
            if self.tab_key(self.sidebar_tab, cx).as_deref() == Some(key) {
                self.sidebar_tab = SidebarTab::Sessions;
            }
        }
        self.save_state(cx);
        cx.notify();
    }

    /// The rail's keys in the user's order, hidden ones included.
    pub(crate) fn rail_keys(&self, cx: &App) -> Vec<String> {
        let panels = self.ext_host.read(cx).panels();
        let default: Vec<String> = (TABS.iter().map(|(_, key, ..)| key.to_string()))
            .chain(panels.iter().map(|p| p.key()))
            .collect();
        ordered(&default, &self.rail_order)
    }
}

#[cfg(test)]
mod tests {
    use super::ordered;

    #[test]
    fn ordered_skips_gone_keys_and_appends_new_ones() {
        let keys = |keys: &[&str]| keys.iter().map(|k| k.to_string()).collect::<Vec<_>>();
        assert_eq!(
            ordered(&keys(&["a", "b", "c", "d"]), &keys(&["c", "x", "a"])),
            keys(&["c", "a", "b", "d"])
        );
    }
}
