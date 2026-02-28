use std::{ops::Range, path::PathBuf, rc::Rc, time::Duration};

use floem::{
    action::exec_after,
    ext_event::create_ext_action,
    keyboard::Modifiers,
    reactive::{Memo, RwSignal, Scope, SignalGet, SignalUpdate, SignalWith},
    views::VirtualVector,
};
use indexmap::IndexMap;
use lapce_core::{mode::Mode, selection::Selection};
use lapce_rpc::proxy::{ProxyResponse, SearchMatch};
use lapce_xi_rope::Rope;

use crate::{
    command::{CommandExecuted, CommandKind},
    editor::EditorData,
    keypress::{KeyPressFocus, condition::Condition},
    main_split::MainSplitData,
    window_tab::CommonData,
};

#[derive(Clone)]
pub struct SearchMatchData {
    pub expanded: RwSignal<bool>,
    pub matches: RwSignal<im::Vector<SearchMatch>>,
    pub line_height: Memo<f64>,
}

impl SearchMatchData {
    pub fn height(&self) -> f64 {
        let line_height = self.line_height.get();
        let count = if self.expanded.get() {
            self.matches.with(|m| m.len()) + 1
        } else {
            1
        };
        line_height * count as f64
    }
}

#[derive(Clone, Debug)]
pub struct GlobalSearchData {
    pub editor: EditorData,
    pub search_result: RwSignal<IndexMap<PathBuf, SearchMatchData>>,
    pub main_split: MainSplitData,
    pub common: Rc<CommonData>,
}

impl KeyPressFocus for GlobalSearchData {
    fn get_mode(&self) -> Mode {
        Mode::Insert
    }

    fn check_condition(&self, condition: Condition) -> bool {
        matches!(condition, Condition::PanelFocus)
    }

    fn run_command(
        &self,
        command: &crate::command::LapceCommand,
        count: Option<usize>,
        mods: Modifiers,
    ) -> CommandExecuted {
        match &command.kind {
            CommandKind::Workbench(_) => {}
            CommandKind::Scroll(_) => {}
            CommandKind::Focus(_) => {}
            CommandKind::Edit(_)
            | CommandKind::Move(_)
            | CommandKind::MultiSelection(_) => {
                return self.editor.run_command(command, count, mods);
            }
            CommandKind::MotionMode(_) => {}
        }
        CommandExecuted::No
    }

    fn receive_char(&self, c: &str) {
        self.editor.receive_char(c);
    }
}

impl VirtualVector<(PathBuf, SearchMatchData)> for GlobalSearchData {
    fn total_len(&self) -> usize {
        self.search_result.with(|result| result.len())
    }

    fn slice(
        &mut self,
        range: Range<usize>,
    ) -> impl Iterator<Item = (PathBuf, SearchMatchData)> {
        let start = range.start;
        let len = range.len();
        self.search_result
            .get()
            .into_iter()
            .skip(start)
            .take(len)
    }
}

impl GlobalSearchData {
    pub fn new(cx: Scope, main_split: MainSplitData) -> Self {
        let common = main_split.common.clone();
        let editor = main_split.editors.make_local(cx, common.clone());
        let search_result = cx.create_rw_signal(IndexMap::new());

        let global_search = Self {
            editor,
            search_result,
            main_split,
            common,
        };

        {
            let global_search = global_search.clone();
            let buffer = global_search.editor.doc().buffer;
            let search_id = cx.create_rw_signal(0u64);
            cx.create_effect(move |_| {
                let pattern = buffer.with(|buffer| buffer.to_string());
                search_id.update(|id| {
                    *id += 1;
                });
                let id = search_id.get_untracked();
                if pattern.is_empty() {
                    global_search.search_result.update(|r| r.clear());
                    return;
                }

                let global_search = global_search.clone();
                exec_after(Duration::from_millis(250), move |_| {
                    if search_id.get_untracked() != id {
                        return;
                    }
                    let case_sensitive = global_search.common.find.case_sensitive(true);
                    let whole_word = global_search.common.find.whole_words.get();
                    let is_regex = global_search.common.find.is_regex.get();
                    let send = {
                        let global_search = global_search.clone();
                        create_ext_action(cx, move |result| {
                            if let Ok(ProxyResponse::GlobalSearchResponse { matches }) =
                                result
                            {
                                global_search.update_matches(matches);
                            }
                        })
                    };
                    global_search.common.proxy.global_search(
                        pattern,
                        case_sensitive,
                        whole_word,
                        is_regex,
                        move |result| {
                            send(result);
                        },
                    );
                });
            });
        }

        {
            let buffer = global_search.editor.doc().buffer;
            let main_split = global_search.main_split.clone();
            cx.create_effect(move |_| {
                let content = buffer.with(|buffer| buffer.to_string());
                main_split.set_find_pattern(Some(content));
            });
        }

        global_search
    }

    fn update_matches(&self, matches: IndexMap<PathBuf, Vec<SearchMatch>>) {
        let mut to_update = Vec::new();

        self.search_result.update(|current| {
            let mut to_remove = Vec::new();
            for path in current.keys() {
                if !matches.contains_key(path) {
                    to_remove.push(path.clone());
                }
            }
            for path in to_remove {
                current.shift_remove(&path);
            }

            for (path, match_list) in matches {
                if let Some(match_data) = current.get(&path) {
                    to_update.push((match_data.matches, match_list.into()));
                } else {
                    let match_data = SearchMatchData {
                        expanded: self.common.scope.create_rw_signal(true),
                        matches: self
                            .common
                            .scope
                            .create_rw_signal(match_list.into()),
                        line_height: self.common.ui_line_height,
                    };
                    current.insert(path, match_data);
                }
            }
        });

        // Update nested signals outside the `update` closure to prevent
        // "RefCell already mutably borrowed" panics when reactive UI updates 
        // try to read `search_result` synchronously.
        for (signal, match_list) in to_update {
            signal.set(match_list);
        }
    }

    pub fn set_pattern(&self, pattern: String) {
        let pattern_len = pattern.len();
        self.editor.doc().reload(Rope::from(pattern), true);
        self.editor
            .cursor()
            .update(|cursor| cursor.set_insert(Selection::region(0, pattern_len)));
    }
}
