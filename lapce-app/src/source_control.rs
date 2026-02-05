use std::{path::PathBuf, rc::Rc};

use floem::{
    ext_event::create_ext_action,
    keyboard::Modifiers,
    reactive::{RwSignal, Scope, SignalUpdate, SignalWith},
};
use indexmap::IndexMap;
use lapce_core::mode::Mode;
use lapce_rpc::source_control::{FileDiff, GitCommitInfo};

use crate::{
    command::{CommandExecuted, CommandKind},
    editor::EditorData,
    keypress::{KeyPressFocus, condition::Condition},
    main_split::Editors,
    window_tab::CommonData,
};

#[derive(Clone, Debug)]
pub struct SourceControlData {
    // VCS modified files & whether they should be included in the next commit
    pub file_diffs: RwSignal<IndexMap<PathBuf, (FileDiff, bool)>>,
    pub branch: RwSignal<String>,
    pub branches: RwSignal<im::Vector<String>>,
    pub tags: RwSignal<im::Vector<String>>,
    pub editor: EditorData,
    pub common: Rc<CommonData>,
    
    // Git log/commit history
    pub commits: RwSignal<im::Vector<GitCommitInfo>>,
    pub commits_loading: RwSignal<bool>,
    pub commits_total_count: RwSignal<usize>,
    
    // Loading indicator for git operations (push/pull/fetch)
    pub git_operation_loading: RwSignal<bool>,
}

impl KeyPressFocus for SourceControlData {
    fn get_mode(&self) -> Mode {
        Mode::Insert
    }

    fn check_condition(&self, condition: Condition) -> bool {
        matches!(
            condition,
            Condition::PanelFocus | Condition::SourceControlFocus
        )
    }

    fn run_command(
        &self,
        command: &crate::command::LapceCommand,
        count: Option<usize>,
        mods: Modifiers,
    ) -> CommandExecuted {
        match &command.kind {
            CommandKind::Edit(_)
            | CommandKind::Move(_)
            | CommandKind::MultiSelection(_) => {
                self.editor.run_command(command, count, mods)
            }
            _ => CommandExecuted::No,
        }
    }

    fn receive_char(&self, c: &str) {
        self.editor.receive_char(c);
    }
}

impl SourceControlData {
    pub fn new(cx: Scope, editors: Editors, common: Rc<CommonData>) -> Self {
        Self {
            // Use the shared file_diffs from CommonData
            file_diffs: common.file_diffs,
            branch: cx.create_rw_signal("".to_string()),
            branches: cx.create_rw_signal(im::Vector::new()),
            tags: cx.create_rw_signal(im::Vector::new()),
            editor: editors.make_local(cx, common.clone()),
            commits: cx.create_rw_signal(im::Vector::new()),
            commits_loading: cx.create_rw_signal(false),
            commits_total_count: cx.create_rw_signal(0),
            git_operation_loading: cx.create_rw_signal(false),
            common,
        }
    }

    pub fn commit(&self) {
        let diffs: Vec<FileDiff> = self.file_diffs.with_untracked(|file_diffs| {
            file_diffs
                .iter()
                .filter_map(
                    |(_, (diff, checked))| {
                        if *checked { Some(diff) } else { None }
                    },
                )
                .cloned()
                .collect()
        });
        if diffs.is_empty() {
            return;
        }

        let message = self
            .editor
            .doc()
            .buffer
            .with_untracked(|buffer| buffer.to_string());
        let message = message.trim();
        if message.is_empty() {
            return;
        }

        self.editor.reset();
        self.common.proxy.git_commit(message.to_string(), diffs);
    }
    
    /// Load git log commits from the repository
    pub fn load_git_log(&self) {
        eprintln!("DEBUG: load_git_log() called - fetching commits...");
        
        let commits = self.commits;
        let commits_loading = self.commits_loading;
        let commits_total_count = self.commits_total_count;
        
        // Set loading state
        commits_loading.set(true);
        
        // Create ext action to handle response on UI thread
        let send = create_ext_action(self.common.scope, move |result: Result<lapce_rpc::proxy::ProxyResponse, lapce_rpc::RpcError>| {
            eprintln!("DEBUG: git_log callback received on UI thread");
            commits_loading.set(false);
            
            match result {
                Ok(response) => {
                    eprintln!("DEBUG: git_log response received");
                    if let lapce_rpc::proxy::ProxyResponse::GitLogResponse { result } = response {
                        eprintln!("DEBUG: Loaded {} commits, total: {}", result.commits.len(), result.total_count);
                        commits.set(result.commits.into_iter().collect());
                        commits_total_count.set(result.total_count);
                    } else {
                        eprintln!("DEBUG: Unexpected response type: {:?}", response);
                    }
                }
                Err(e) => {
                    eprintln!("DEBUG: git_log error: {:?}", e);
                }
            }
        });
        
        // Fetch git log with limit of 100 commits, no skip, current branch
        self.common.proxy.git_log(
            100,  // limit
            0,    // skip
            None, // branch (None = current branch)
            None, // author filter
            None, // search filter
            send,
        );
    }
}
