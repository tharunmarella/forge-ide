use std::{
    collections::{HashMap, HashSet},
    fs, io,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    thread,
    time::Duration,
};

use alacritty_terminal::{event::WindowSize, event_loop::Msg};
use anyhow::{Context, Result, anyhow};
use crossbeam_channel::Sender;
use git2::{
    DiffOptions, ErrorCode::NotFound, Oid, Repository, build::CheckoutBuilder,
};
use grep_matcher::Matcher;
use grep_regex::RegexMatcherBuilder;
use grep_searcher::{SearcherBuilder, sinks::UTF8};
use indexmap::IndexMap;
use lapce_rpc::{
    RequestId, RpcError,
    buffer::BufferId,
    core::{CoreNotification, CoreRpcHandler, FileChanged},
    file::FileNodeItem,
    file_line::FileLine,
    proxy::{
        ProxyHandler, ProxyNotification, ProxyRequest, ProxyResponse,
        ProxyRpcHandler, SearchMatch,
    },
    source_control::{DiffInfo, FileDiff},
    style::{LineStyle, SemanticStyles},
    terminal::TermId,
};
use lapce_xi_rope::Rope;
use lsp_types::{
    CancelParams, MessageType, NumberOrString, Position, Range, ShowMessageParams,
    TextDocumentItem, Url,
    notification::{Cancel, Notification},
};
use parking_lot::Mutex;

use crate::{
    buffer::{Buffer, get_mod_time, load_file},
    plugin::{PluginCatalogRpcHandler, catalog::PluginCatalog},
    terminal::{Terminal, TerminalSender},
    watcher::{FileWatcher, Notify, WatchToken},
};

const OPEN_FILE_EVENT_TOKEN: WatchToken = WatchToken(1);
const WORKSPACE_EVENT_TOKEN: WatchToken = WatchToken(2);

pub struct Dispatcher {
    workspace: Option<PathBuf>,
    pub proxy_rpc: ProxyRpcHandler,
    core_rpc: CoreRpcHandler,
    catalog_rpc: PluginCatalogRpcHandler,
    buffers: HashMap<PathBuf, Buffer>,
    terminals: HashMap<TermId, TerminalSender>,
    file_watcher: FileWatcher,
    window_id: usize,
    tab_id: usize,
}

impl ProxyHandler for Dispatcher {
    fn handle_notification(&mut self, rpc: ProxyNotification) {
        use ProxyNotification::*;
        match rpc {
            Initialize {
                workspace,
                disabled_volts,
                extra_plugin_paths,
                plugin_configurations,
                window_id,
                tab_id,
            } => {
                self.window_id = window_id;
                self.tab_id = tab_id;
                self.workspace = workspace;
                self.file_watcher.notify(FileWatchNotifier::new(
                    self.workspace.clone(),
                    self.core_rpc.clone(),
                    self.proxy_rpc.clone(),
                ));
                if let Some(workspace) = self.workspace.as_ref() {
                    self.file_watcher
                        .watch(workspace, true, WORKSPACE_EVENT_TOKEN);
                }

                let plugin_rpc = self.catalog_rpc.clone();
                let workspace = self.workspace.clone();
                thread::spawn(move || {
                    let mut plugin = PluginCatalog::new(
                        workspace,
                        disabled_volts,
                        extra_plugin_paths,
                        plugin_configurations,
                        plugin_rpc.clone(),
                    );
                    plugin_rpc.mainloop(&mut plugin);
                });
                self.core_rpc.notification(CoreNotification::ProxyStatus {
                    status: lapce_rpc::proxy::ProxyStatus::Connected,
                });

                // send home directory for initinal filepicker dir
                let dirs = directories::UserDirs::new();

                if let Some(dirs) = dirs {
                    self.core_rpc.home_dir(dirs.home_dir().into());
                }
            }
            OpenPaths { paths } => {
                self.core_rpc
                    .notification(CoreNotification::OpenPaths { paths });
            }
            OpenFileChanged { path } => {
                if path.exists() {
                    if let Some(buffer) = self.buffers.get(&path) {
                        if get_mod_time(&buffer.path) == buffer.mod_time {
                            return;
                        }
                        match load_file(&buffer.path) {
                            Ok(content) => {
                                self.core_rpc.open_file_changed(
                                    path,
                                    FileChanged::Change(content),
                                );
                            }
                            Err(err) => {
                                tracing::event!(
                                    tracing::Level::ERROR,
                                    "Failed to re-read file after change notification: {err}"
                                );
                            }
                        }
                    }
                } else {
                    self.buffers.remove(&path);
                    self.core_rpc.open_file_changed(path, FileChanged::Delete);
                }
            }
            Completion {
                request_id,
                path,
                input,
                position,
            } => {
                self.catalog_rpc
                    .completion(request_id, &path, input, position);
            }
            SignatureHelp {
                request_id,
                path,
                position,
            } => {
                self.catalog_rpc.signature_help(request_id, &path, position);
            }
            Shutdown {} => {
                self.catalog_rpc.shutdown();
                for (_, sender) in self.terminals.iter() {
                    sender.send(Msg::Shutdown);
                }
                self.proxy_rpc.shutdown();
            }
            Update { path, delta, rev } => {
                let buffer = self.buffers.get_mut(&path).unwrap();
                let old_text = buffer.rope.clone();
                buffer.update(&delta, rev);
                self.catalog_rpc.did_change_text_document(
                    &path,
                    rev,
                    delta,
                    old_text,
                    buffer.rope.clone(),
                );
            }
            UpdatePluginConfigs { configs } => {
                if let Err(err) = self.catalog_rpc.update_plugin_configs(configs) {
                    tracing::error!("{:?}", err);
                }
            }
            NewTerminal { term_id, profile } => {
                let mut terminal = match Terminal::new(term_id, profile, 50, 10) {
                    Ok(terminal) => terminal,
                    Err(e) => {
                        self.core_rpc.terminal_launch_failed(term_id, e.to_string());
                        return;
                    }
                };

                #[allow(unused)]
                let mut child_id = None;

                #[cfg(target_os = "windows")]
                {
                    child_id = terminal.pty.child_watcher().pid().map(|x| x.get());
                }
                #[cfg(not(target_os = "windows"))]
                {
                    child_id = Some(terminal.pty.child().id());
                }

                self.core_rpc.terminal_process_id(term_id, child_id);
                let tx = terminal.tx.clone();
                let poller = terminal.poller.clone();
                let sender = TerminalSender::new(tx, poller);
                self.terminals.insert(term_id, sender);
                let rpc = self.core_rpc.clone();
                thread::spawn(move || {
                    terminal.run(rpc);
                });
            }
            TerminalWrite { term_id, content } => {
                if let Some(tx) = self.terminals.get(&term_id) {
                    tx.send(Msg::Input(content.into_bytes().into()));
                }
            }
            TerminalResize {
                term_id,
                width,
                height,
            } => {
                if let Some(tx) = self.terminals.get(&term_id) {
                    let size = WindowSize {
                        num_lines: height as u16,
                        num_cols: width as u16,
                        cell_width: 1,
                        cell_height: 1,
                    };

                    tx.send(Msg::Resize(size));
                }
            }
            TerminalClose { term_id } => {
                if let Some(tx) = self.terminals.remove(&term_id) {
                    tx.send(Msg::Shutdown);
                }
            }
            DapStart {
                config,
                breakpoints,
            } => {
                if let Err(err) = self.catalog_rpc.dap_start(config, breakpoints) {
                    tracing::error!("{:?}", err);
                }
            }
            DapProcessId {
                dap_id,
                process_id,
                term_id,
            } => {
                if let Err(err) =
                    self.catalog_rpc.dap_process_id(dap_id, process_id, term_id)
                {
                    tracing::error!("{:?}", err);
                }
            }
            DapContinue { dap_id, thread_id } => {
                if let Err(err) = self.catalog_rpc.dap_continue(dap_id, thread_id) {
                    tracing::error!("{:?}", err);
                }
            }
            DapPause { dap_id, thread_id } => {
                if let Err(err) = self.catalog_rpc.dap_pause(dap_id, thread_id) {
                    tracing::error!("{:?}", err);
                }
            }
            DapStepOver { dap_id, thread_id } => {
                if let Err(err) = self.catalog_rpc.dap_step_over(dap_id, thread_id) {
                    tracing::error!("{:?}", err);
                }
            }
            DapStepInto { dap_id, thread_id } => {
                if let Err(err) = self.catalog_rpc.dap_step_into(dap_id, thread_id) {
                    tracing::error!("{:?}", err);
                }
            }
            DapStepOut { dap_id, thread_id } => {
                if let Err(err) = self.catalog_rpc.dap_step_out(dap_id, thread_id) {
                    tracing::error!("{:?}", err);
                }
            }
            DapStop { dap_id } => {
                if let Err(err) = self.catalog_rpc.dap_stop(dap_id) {
                    tracing::error!("{:?}", err);
                }
            }
            DapDisconnect { dap_id } => {
                if let Err(err) = self.catalog_rpc.dap_disconnect(dap_id) {
                    tracing::error!("{:?}", err);
                }
            }
            DapRestart {
                dap_id,
                breakpoints,
            } => {
                if let Err(err) = self.catalog_rpc.dap_restart(dap_id, breakpoints) {
                    tracing::error!("{:?}", err);
                }
            }
            DapSetBreakpoints {
                dap_id,
                path,
                breakpoints,
            } => {
                if let Err(err) =
                    self.catalog_rpc
                        .dap_set_breakpoints(dap_id, path, breakpoints)
                {
                    tracing::error!("{:?}", err);
                }
            }
            InstallVolt { volt } => {
                let catalog_rpc = self.catalog_rpc.clone();
                if let Err(err) = catalog_rpc.install_volt(volt) {
                    tracing::error!("{:?}", err);
                }
            }
            ReloadVolt { volt } => {
                if let Err(err) = self.catalog_rpc.reload_volt(volt) {
                    tracing::error!("{:?}", err);
                }
            }
            RemoveVolt { volt } => {
                self.catalog_rpc.remove_volt(volt);
            }
            DisableVolt { volt } => {
                self.catalog_rpc.stop_volt(volt);
            }
            EnableVolt { volt } => {
                if let Err(err) = self.catalog_rpc.enable_volt(volt) {
                    tracing::error!("{:?}", err);
                }
            }
            GitCommit { message, diffs } => {
                if let Some(workspace) = self.workspace.as_ref() {
                    match git_commit(workspace, &message, diffs) {
                        Ok(()) => (),
                        Err(e) => {
                            self.core_rpc.show_message(
                                "Git Commit failure".to_owned(),
                                ShowMessageParams {
                                    typ: MessageType::ERROR,
                                    message: e.to_string(),
                                },
                            );
                        }
                    }
                }
            }
            GitCheckout { reference } => {
                if let Some(workspace) = self.workspace.as_ref() {
                    match git_checkout(workspace, &reference) {
                        Ok(()) => (),
                        Err(e) => eprintln!("{e:?}"),
                    }
                }
            }
            GitDiscardFilesChanges { files } => {
                if let Some(workspace) = self.workspace.as_ref() {
                    match git_discard_files_changes(
                        workspace,
                        files.iter().map(AsRef::as_ref),
                    ) {
                        Ok(()) => (),
                        Err(e) => eprintln!("{e:?}"),
                    }
                }
            }
            GitDiscardWorkspaceChanges {} => {
                if let Some(workspace) = self.workspace.as_ref() {
                    match git_discard_workspace_changes(workspace) {
                        Ok(()) => (),
                        Err(e) => eprintln!("{e:?}"),
                    }
                }
            }
            GitInit {} => {
                if let Some(workspace) = self.workspace.as_ref() {
                    match git_init(workspace) {
                        Ok(()) => (),
                        Err(e) => eprintln!("{e:?}"),
                    }
                }
            }
            LspCancel { id } => {
                self.catalog_rpc.send_notification(
                    None,
                    Cancel::METHOD,
                    CancelParams {
                        id: NumberOrString::Number(id),
                    },
                    None,
                    None,
                    false,
                );
            }
        }
    }

    fn handle_request(&mut self, id: RequestId, rpc: ProxyRequest) {
        use ProxyRequest::*;
        match rpc {
            NewBuffer { buffer_id, path } => {
                let buffer = Buffer::new(buffer_id, path.clone());
                let content = buffer.rope.to_string();
                let read_only = buffer.read_only;
                self.catalog_rpc.did_open_document(
                    &path,
                    buffer.language_id.to_string(),
                    buffer.rev as i32,
                    content.clone(),
                );
                self.file_watcher.watch(&path, false, OPEN_FILE_EVENT_TOKEN);
                self.buffers.insert(path, buffer);
                self.respond_rpc(
                    id,
                    Ok(ProxyResponse::NewBufferResponse { content, read_only }),
                );
            }
            BufferHead { path } => {
                let result = if let Some(workspace) = self.workspace.as_ref() {
                    let result = file_get_head(workspace, &path);
                    if let Ok((_blob_id, content)) = result {
                        Ok(ProxyResponse::BufferHeadResponse {
                            version: "head".to_string(),
                            content,
                        })
                    } else {
                        Err(RpcError {
                            code: 0,
                            message: "can't get file head".to_string(),
                        })
                    }
                } else {
                    Err(RpcError {
                        code: 0,
                        message: "no workspace set".to_string(),
                    })
                };
                self.respond_rpc(id, result);
            }
            GlobalSearch {
                pattern,
                case_sensitive,
                whole_word,
                is_regex,
            } => {
                static WORKER_ID: AtomicU64 = AtomicU64::new(0);
                let our_id = WORKER_ID.fetch_add(1, Ordering::SeqCst) + 1;

                let workspace = self.workspace.clone();
                let buffers = self
                    .buffers
                    .iter()
                    .map(|p| p.0)
                    .cloned()
                    .collect::<Vec<PathBuf>>();
                let proxy_rpc = self.proxy_rpc.clone();

                // Perform the search on another thread to avoid blocking the proxy thread
                thread::spawn(move || {
                    proxy_rpc.handle_response(
                        id,
                        search_in_path(
                            our_id,
                            &WORKER_ID,
                            workspace
                                .iter()
                                .flat_map(|w| ignore::Walk::new(w).flatten())
                                .chain(
                                    buffers.iter().flat_map(|p| {
                                        ignore::Walk::new(p).flatten()
                                    }),
                                )
                                .map(|p| p.into_path()),
                            &pattern,
                            case_sensitive,
                            whole_word,
                            is_regex,
                        ),
                    );
                });
            }
            CompletionResolve {
                plugin_id,
                completion_item,
            } => {
                let proxy_rpc = self.proxy_rpc.clone();
                self.catalog_rpc.completion_resolve(
                    plugin_id,
                    *completion_item,
                    move |result| {
                        let result = result.map(|item| {
                            ProxyResponse::CompletionResolveResponse {
                                item: Box::new(item),
                            }
                        });
                        proxy_rpc.handle_response(id, result);
                    },
                );
            }
            GetHover {
                request_id,
                path,
                position,
            } => {
                let proxy_rpc = self.proxy_rpc.clone();
                self.catalog_rpc.hover(&path, position, move |_, result| {
                    let result = result.map(|hover| ProxyResponse::HoverResponse {
                        request_id,
                        hover,
                    });
                    proxy_rpc.handle_response(id, result);
                });
            }
            GetSignature { .. } => {}
            GetReferences { path, position } => {
                let proxy_rpc = self.proxy_rpc.clone();
                self.catalog_rpc.get_references(
                    &path,
                    position,
                    move |_, result| {
                        let result = result.map(|references| {
                            ProxyResponse::GetReferencesResponse { references }
                        });
                        proxy_rpc.handle_response(id, result);
                    },
                );
            }
            GitGetRemoteFileUrl { file } => {
                if let Some(workspace) = self.workspace.as_ref() {
                    match git_get_remote_file_url(workspace, &file) {
                        Ok(s) => self.proxy_rpc.handle_response(
                            id,
                            Ok(ProxyResponse::GitGetRemoteFileUrl { file_url: s }),
                        ),
                        Err(e) => eprintln!("{e:?}"),
                    }
                }
            }
            GitLog { limit, skip, branch, author, search } => {
                let result = if let Some(workspace) = self.workspace.as_ref() {
                    match git_log(workspace, limit, skip, branch, author, search) {
                        Ok(result) => Ok(ProxyResponse::GitLogResponse { result }),
                        Err(e) => Err(RpcError {
                            code: 0,
                            message: format!("git log error: {}", e),
                        }),
                    }
                } else {
                    Err(RpcError {
                        code: 0,
                        message: "no workspace set".to_string(),
                    })
                };
                self.respond_rpc(id, result);
            }
            // Git Branch Operations
            GitListBranches { include_remote } => {
                let result = if let Some(workspace) = self.workspace.as_ref() {
                    match git_list_branches(workspace, include_remote) {
                        Ok(branches) => Ok(ProxyResponse::GitBranchListResponse { branches }),
                        Err(e) => Err(RpcError { code: 0, message: format!("git list branches error: {}", e) }),
                    }
                } else {
                    Err(RpcError { code: 0, message: "no workspace set".to_string() })
                };
                self.respond_rpc(id, result);
            }
            GitCreateBranch { name, start_point, checkout } => {
                let result = if let Some(workspace) = self.workspace.as_ref() {
                    match git_create_branch(workspace, &name, start_point.as_deref(), checkout) {
                        Ok(result) => Ok(ProxyResponse::GitBranchResponse { result }),
                        Err(e) => Err(RpcError { code: 0, message: format!("git create branch error: {}", e) }),
                    }
                } else {
                    Err(RpcError { code: 0, message: "no workspace set".to_string() })
                };
                self.respond_rpc(id, result);
            }
            GitDeleteBranch { name, force, delete_remote } => {
                let result = if let Some(workspace) = self.workspace.as_ref() {
                    match git_delete_branch(workspace, &name, force, delete_remote) {
                        Ok(result) => Ok(ProxyResponse::GitBranchResponse { result }),
                        Err(e) => Err(RpcError { code: 0, message: format!("git delete branch error: {}", e) }),
                    }
                } else {
                    Err(RpcError { code: 0, message: "no workspace set".to_string() })
                };
                self.respond_rpc(id, result);
            }
            GitRenameBranch { old_name, new_name } => {
                let result = if let Some(workspace) = self.workspace.as_ref() {
                    match git_rename_branch(workspace, &old_name, &new_name) {
                        Ok(result) => Ok(ProxyResponse::GitBranchResponse { result }),
                        Err(e) => Err(RpcError { code: 0, message: format!("git rename branch error: {}", e) }),
                    }
                } else {
                    Err(RpcError { code: 0, message: "no workspace set".to_string() })
                };
                self.respond_rpc(id, result);
            }
            // Git Push/Pull/Fetch - These require network operations, placeholder for now
            GitPush { options: _ } => {
                // TODO: Implement push using git2 or command-line git
                self.respond_rpc(id, Err(RpcError { 
                    code: 0, 
                    message: "Push requires SSH/HTTPS credentials - use terminal for now".to_string() 
                }));
            }
            GitPull { options: _ } => {
                // TODO: Implement pull using git2 or command-line git
                self.respond_rpc(id, Err(RpcError { 
                    code: 0, 
                    message: "Pull requires SSH/HTTPS credentials - use terminal for now".to_string() 
                }));
            }
            GitFetch { options: _ } => {
                // TODO: Implement fetch using git2 or command-line git
                self.respond_rpc(id, Err(RpcError { 
                    code: 0, 
                    message: "Fetch requires SSH/HTTPS credentials - use terminal for now".to_string() 
                }));
            }
            // Git Stash Operations
            GitStashList {} => {
                let result = if let Some(workspace) = self.workspace.as_ref() {
                    match git_stash_list(workspace) {
                        Ok(result) => Ok(ProxyResponse::GitStashListResponse { result }),
                        Err(e) => Err(RpcError { code: 0, message: format!("git stash list error: {}", e) }),
                    }
                } else {
                    Err(RpcError { code: 0, message: "no workspace set".to_string() })
                };
                self.respond_rpc(id, result);
            }
            GitStashSave { message, include_untracked, keep_index } => {
                let result = if let Some(workspace) = self.workspace.as_ref() {
                    match git_stash_save(workspace, message.as_deref(), include_untracked, keep_index) {
                        Ok(result) => Ok(ProxyResponse::GitStashOpResponse { result }),
                        Err(e) => Err(RpcError { code: 0, message: format!("git stash save error: {}", e) }),
                    }
                } else {
                    Err(RpcError { code: 0, message: "no workspace set".to_string() })
                };
                self.respond_rpc(id, result);
            }
            GitStashPop { index } => {
                let result = if let Some(workspace) = self.workspace.as_ref() {
                    match git_stash_pop(workspace, index) {
                        Ok(result) => Ok(ProxyResponse::GitStashOpResponse { result }),
                        Err(e) => Err(RpcError { code: 0, message: format!("git stash pop error: {}", e) }),
                    }
                } else {
                    Err(RpcError { code: 0, message: "no workspace set".to_string() })
                };
                self.respond_rpc(id, result);
            }
            GitStashApply { index } => {
                let result = if let Some(workspace) = self.workspace.as_ref() {
                    match git_stash_apply(workspace, index) {
                        Ok(result) => Ok(ProxyResponse::GitStashOpResponse { result }),
                        Err(e) => Err(RpcError { code: 0, message: format!("git stash apply error: {}", e) }),
                    }
                } else {
                    Err(RpcError { code: 0, message: "no workspace set".to_string() })
                };
                self.respond_rpc(id, result);
            }
            GitStashDrop { index } => {
                let result = if let Some(workspace) = self.workspace.as_ref() {
                    match git_stash_drop(workspace, index) {
                        Ok(result) => Ok(ProxyResponse::GitStashOpResponse { result }),
                        Err(e) => Err(RpcError { code: 0, message: format!("git stash drop error: {}", e) }),
                    }
                } else {
                    Err(RpcError { code: 0, message: "no workspace set".to_string() })
                };
                self.respond_rpc(id, result);
            }
            // Git Merge Operations
            GitMerge { options } => {
                let result = if let Some(workspace) = self.workspace.as_ref() {
                    match git_merge(workspace, &options) {
                        Ok(result) => Ok(ProxyResponse::GitMergeResponse { result }),
                        Err(e) => Err(RpcError { code: 0, message: format!("git merge error: {}", e) }),
                    }
                } else {
                    Err(RpcError { code: 0, message: "no workspace set".to_string() })
                };
                self.respond_rpc(id, result);
            }
            GitMergeAbort {} => {
                let result = if let Some(workspace) = self.workspace.as_ref() {
                    match git_merge_abort(workspace) {
                        Ok(result) => Ok(ProxyResponse::GitMergeResponse { result }),
                        Err(e) => Err(RpcError { code: 0, message: format!("git merge abort error: {}", e) }),
                    }
                } else {
                    Err(RpcError { code: 0, message: "no workspace set".to_string() })
                };
                self.respond_rpc(id, result);
            }
            // Git Rebase Operations - Complex, placeholder for now
            GitRebase { options: _ } => {
                self.respond_rpc(id, Err(RpcError { 
                    code: 0, 
                    message: "Interactive rebase not yet implemented - use terminal".to_string() 
                }));
            }
            GitRebaseAction { action: _ } => {
                self.respond_rpc(id, Err(RpcError { 
                    code: 0, 
                    message: "Rebase actions not yet implemented - use terminal".to_string() 
                }));
            }
            // Git Cherry-pick - Placeholder
            GitCherryPick { options: _ } => {
                self.respond_rpc(id, Err(RpcError { 
                    code: 0, 
                    message: "Cherry-pick not yet implemented - use terminal".to_string() 
                }));
            }
            GitCherryPickAction { action: _ } => {
                self.respond_rpc(id, Err(RpcError { 
                    code: 0, 
                    message: "Cherry-pick actions not yet implemented - use terminal".to_string() 
                }));
            }
            // Git Reset
            GitReset { options } => {
                let result = if let Some(workspace) = self.workspace.as_ref() {
                    match git_reset(workspace, &options) {
                        Ok(result) => Ok(ProxyResponse::GitResetResponse { result }),
                        Err(e) => Err(RpcError { code: 0, message: format!("git reset error: {}", e) }),
                    }
                } else {
                    Err(RpcError { code: 0, message: "no workspace set".to_string() })
                };
                self.respond_rpc(id, result);
            }
            // Git Revert - Placeholder
            GitRevert { options: _ } => {
                self.respond_rpc(id, Err(RpcError { 
                    code: 0, 
                    message: "Revert not yet implemented - use terminal".to_string() 
                }));
            }
            GitRevertAction { action: _ } => {
                self.respond_rpc(id, Err(RpcError { 
                    code: 0, 
                    message: "Revert actions not yet implemented - use terminal".to_string() 
                }));
            }
            // Git Blame
            GitBlame { path, commit } => {
                let result = if let Some(workspace) = self.workspace.as_ref() {
                    match git_blame(workspace, &path, commit.as_deref()) {
                        Ok(result) => Ok(ProxyResponse::GitBlameResponse { result }),
                        Err(e) => Err(RpcError { code: 0, message: format!("git blame error: {}", e) }),
                    }
                } else {
                    Err(RpcError { code: 0, message: "no workspace set".to_string() })
                };
                self.respond_rpc(id, result);
            }
            // Git Tags
            GitListTags {} => {
                let result = if let Some(workspace) = self.workspace.as_ref() {
                    match git_list_tags(workspace) {
                        Ok(tags) => Ok(ProxyResponse::GitTagListResponse { tags }),
                        Err(e) => Err(RpcError { code: 0, message: format!("git list tags error: {}", e) }),
                    }
                } else {
                    Err(RpcError { code: 0, message: "no workspace set".to_string() })
                };
                self.respond_rpc(id, result);
            }
            GitCreateTag { name, target, message, annotated } => {
                let result = if let Some(workspace) = self.workspace.as_ref() {
                    match git_create_tag(workspace, &name, target.as_deref(), message.as_deref(), annotated) {
                        Ok(result) => Ok(ProxyResponse::GitTagOpResponse { result }),
                        Err(e) => Err(RpcError { code: 0, message: format!("git create tag error: {}", e) }),
                    }
                } else {
                    Err(RpcError { code: 0, message: "no workspace set".to_string() })
                };
                self.respond_rpc(id, result);
            }
            GitDeleteTag { name, delete_remote: _ } => {
                let result = if let Some(workspace) = self.workspace.as_ref() {
                    match git_delete_tag(workspace, &name) {
                        Ok(result) => Ok(ProxyResponse::GitTagOpResponse { result }),
                        Err(e) => Err(RpcError { code: 0, message: format!("git delete tag error: {}", e) }),
                    }
                } else {
                    Err(RpcError { code: 0, message: "no workspace set".to_string() })
                };
                self.respond_rpc(id, result);
            }
            // Git Remotes
            GitListRemotes {} => {
                let result = if let Some(workspace) = self.workspace.as_ref() {
                    match git_list_remotes(workspace) {
                        Ok(result) => Ok(ProxyResponse::GitRemoteListResponse { result }),
                        Err(e) => Err(RpcError { code: 0, message: format!("git list remotes error: {}", e) }),
                    }
                } else {
                    Err(RpcError { code: 0, message: "no workspace set".to_string() })
                };
                self.respond_rpc(id, result);
            }
            GitAddRemote { name, url } => {
                let result = if let Some(workspace) = self.workspace.as_ref() {
                    match git_add_remote(workspace, &name, &url) {
                        Ok(result) => Ok(ProxyResponse::GitRemoteOpResponse { result }),
                        Err(e) => Err(RpcError { code: 0, message: format!("git add remote error: {}", e) }),
                    }
                } else {
                    Err(RpcError { code: 0, message: "no workspace set".to_string() })
                };
                self.respond_rpc(id, result);
            }
            GitRemoveRemote { name } => {
                let result = if let Some(workspace) = self.workspace.as_ref() {
                    match git_remove_remote(workspace, &name) {
                        Ok(result) => Ok(ProxyResponse::GitRemoteOpResponse { result }),
                        Err(e) => Err(RpcError { code: 0, message: format!("git remove remote error: {}", e) }),
                    }
                } else {
                    Err(RpcError { code: 0, message: "no workspace set".to_string() })
                };
                self.respond_rpc(id, result);
            }
            // Git Status
            GitGetStatus {} => {
                let result = if let Some(workspace) = self.workspace.as_ref() {
                    match git_get_status(workspace) {
                        Ok(result) => Ok(ProxyResponse::GitStatusResponse { result }),
                        Err(e) => Err(RpcError { code: 0, message: format!("git status error: {}", e) }),
                    }
                } else {
                    Err(RpcError { code: 0, message: "no workspace set".to_string() })
                };
                self.respond_rpc(id, result);
            }
            // Git Diff - Placeholder
            GitGetCommitDiff { commit: _ } => {
                self.respond_rpc(id, Err(RpcError { 
                    code: 0, 
                    message: "Commit diff not yet implemented".to_string() 
                }));
            }
            GitGetFileDiff { path: _, staged: _ } => {
                self.respond_rpc(id, Err(RpcError { 
                    code: 0, 
                    message: "File diff not yet implemented".to_string() 
                }));
            }
            // Git Stage
            GitStageFiles { paths } => {
                let result = if let Some(workspace) = self.workspace.as_ref() {
                    match git_stage_files(workspace, &paths) {
                        Ok(()) => Ok(ProxyResponse::GitStageResponse { success: true, message: "Files staged".to_string() }),
                        Err(e) => Err(RpcError { code: 0, message: format!("git stage error: {}", e) }),
                    }
                } else {
                    Err(RpcError { code: 0, message: "no workspace set".to_string() })
                };
                self.respond_rpc(id, result);
            }
            GitUnstageFiles { paths } => {
                let result = if let Some(workspace) = self.workspace.as_ref() {
                    match git_unstage_files(workspace, &paths) {
                        Ok(()) => Ok(ProxyResponse::GitStageResponse { success: true, message: "Files unstaged".to_string() }),
                        Err(e) => Err(RpcError { code: 0, message: format!("git unstage error: {}", e) }),
                    }
                } else {
                    Err(RpcError { code: 0, message: "no workspace set".to_string() })
                };
                self.respond_rpc(id, result);
            }
            GitStageAll {} => {
                let result = if let Some(workspace) = self.workspace.as_ref() {
                    match git_stage_all(workspace) {
                        Ok(()) => Ok(ProxyResponse::GitStageResponse { success: true, message: "All files staged".to_string() }),
                        Err(e) => Err(RpcError { code: 0, message: format!("git stage all error: {}", e) }),
                    }
                } else {
                    Err(RpcError { code: 0, message: "no workspace set".to_string() })
                };
                self.respond_rpc(id, result);
            }
            GitUnstageAll {} => {
                let result = if let Some(workspace) = self.workspace.as_ref() {
                    match git_unstage_all(workspace) {
                        Ok(()) => Ok(ProxyResponse::GitStageResponse { success: true, message: "All files unstaged".to_string() }),
                        Err(e) => Err(RpcError { code: 0, message: format!("git unstage all error: {}", e) }),
                    }
                } else {
                    Err(RpcError { code: 0, message: "no workspace set".to_string() })
                };
                self.respond_rpc(id, result);
            }
            GetDefinition {
                request_id,
                path,
                position,
            } => {
                let proxy_rpc = self.proxy_rpc.clone();
                self.catalog_rpc.get_definition(
                    &path,
                    position,
                    move |_, result| {
                        let result = result.map(|definition| {
                            ProxyResponse::GetDefinitionResponse {
                                request_id,
                                definition,
                            }
                        });
                        proxy_rpc.handle_response(id, result);
                    },
                );
            }
            GetTypeDefinition {
                request_id,
                path,
                position,
            } => {
                let proxy_rpc = self.proxy_rpc.clone();
                self.catalog_rpc.get_type_definition(
                    &path,
                    position,
                    move |_, result| {
                        let result = result.map(|definition| {
                            ProxyResponse::GetTypeDefinition {
                                request_id,
                                definition,
                            }
                        });
                        proxy_rpc.handle_response(id, result);
                    },
                );
            }
            ShowCallHierarchy { path, position } => {
                let proxy_rpc = self.proxy_rpc.clone();
                self.catalog_rpc.show_call_hierarchy(
                    &path,
                    position,
                    move |_, result| {
                        let result = result.map(|items| {
                            ProxyResponse::ShowCallHierarchyResponse { items }
                        });
                        proxy_rpc.handle_response(id, result);
                    },
                );
            }
            CallHierarchyIncoming {
                path,
                call_hierarchy_item,
            } => {
                let proxy_rpc = self.proxy_rpc.clone();
                self.catalog_rpc.call_hierarchy_incoming(
                    &path,
                    call_hierarchy_item,
                    move |_, result| {
                        let result = result.map(|items| {
                            ProxyResponse::CallHierarchyIncomingResponse { items }
                        });
                        proxy_rpc.handle_response(id, result);
                    },
                );
            }
            GetInlayHints { path } => {
                let proxy_rpc = self.proxy_rpc.clone();
                let buffer = self.buffers.get(&path).unwrap();
                let range = Range {
                    start: Position::new(0, 0),
                    end: buffer.offset_to_position(buffer.len()),
                };
                self.catalog_rpc
                    .get_inlay_hints(&path, range, move |_, result| {
                        let result = result
                            .map(|hints| ProxyResponse::GetInlayHints { hints });
                        proxy_rpc.handle_response(id, result);
                    });
            }
            GetInlineCompletions {
                path,
                position,
                trigger_kind,
            } => {
                let proxy_rpc = self.proxy_rpc.clone();
                self.catalog_rpc.get_inline_completions(
                    &path,
                    position,
                    trigger_kind,
                    move |_, result| {
                        let result = result.map(|completions| {
                            ProxyResponse::GetInlineCompletions { completions }
                        });
                        proxy_rpc.handle_response(id, result);
                    },
                );
            }
            GetSemanticTokens { path } => {
                let buffer = self.buffers.get(&path).unwrap();
                let text = buffer.rope.clone();
                let rev = buffer.rev;
                let len = buffer.len();
                let local_path = path.clone();
                let proxy_rpc = self.proxy_rpc.clone();
                let catalog_rpc = self.catalog_rpc.clone();

                let handle_tokens =
                    move |result: Result<Vec<LineStyle>, RpcError>| match result {
                        Ok(styles) => {
                            proxy_rpc.handle_response(
                                id,
                                Ok(ProxyResponse::GetSemanticTokens {
                                    styles: SemanticStyles {
                                        rev,
                                        path: local_path,
                                        styles,
                                        len,
                                    },
                                }),
                            );
                        }
                        Err(e) => {
                            proxy_rpc.handle_response(id, Err(e));
                        }
                    };

                let proxy_rpc = self.proxy_rpc.clone();
                self.catalog_rpc.get_semantic_tokens(
                    &path,
                    move |plugin_id, result| match result {
                        Ok(result) => {
                            catalog_rpc.format_semantic_tokens(
                                plugin_id,
                                result,
                                text,
                                Box::new(handle_tokens),
                            );
                        }
                        Err(e) => {
                            proxy_rpc.handle_response(id, Err(e));
                        }
                    },
                );
            }
            GetCodeActions {
                path,
                position,
                diagnostics,
            } => {
                let proxy_rpc = self.proxy_rpc.clone();
                self.catalog_rpc.get_code_actions(
                    &path,
                    position,
                    diagnostics,
                    move |plugin_id, result| {
                        let result = result.map(|resp| {
                            ProxyResponse::GetCodeActionsResponse { plugin_id, resp }
                        });
                        proxy_rpc.handle_response(id, result);
                    },
                );
            }
            GetDocumentSymbols { path } => {
                let proxy_rpc = self.proxy_rpc.clone();
                self.catalog_rpc
                    .get_document_symbols(&path, move |_, result| {
                        let result = result
                            .map(|resp| ProxyResponse::GetDocumentSymbols { resp });
                        proxy_rpc.handle_response(id, result);
                    });
            }
            GetWorkspaceSymbols { query } => {
                let proxy_rpc = self.proxy_rpc.clone();
                self.catalog_rpc
                    .get_workspace_symbols(query, move |_, result| {
                        let result = result.map(|symbols| {
                            ProxyResponse::GetWorkspaceSymbols { symbols }
                        });
                        proxy_rpc.handle_response(id, result);
                    });
            }
            GetDocumentFormatting { path } => {
                let proxy_rpc = self.proxy_rpc.clone();
                self.catalog_rpc
                    .get_document_formatting(&path, move |_, result| {
                        let result = result.map(|edits| {
                            ProxyResponse::GetDocumentFormatting { edits }
                        });
                        proxy_rpc.handle_response(id, result);
                    });
            }
            PrepareRename { path, position } => {
                let proxy_rpc = self.proxy_rpc.clone();
                self.catalog_rpc.prepare_rename(
                    &path,
                    position,
                    move |_, result| {
                        let result =
                            result.map(|resp| ProxyResponse::PrepareRename { resp });
                        proxy_rpc.handle_response(id, result);
                    },
                );
            }
            Rename {
                path,
                position,
                new_name,
            } => {
                let proxy_rpc = self.proxy_rpc.clone();
                self.catalog_rpc.rename(
                    &path,
                    position,
                    new_name,
                    move |_, result| {
                        let result =
                            result.map(|edit| ProxyResponse::Rename { edit });
                        proxy_rpc.handle_response(id, result);
                    },
                );
            }
            GetFiles { .. } => {
                let workspace = self.workspace.clone();
                let proxy_rpc = self.proxy_rpc.clone();
                thread::spawn(move || {
                    let result = if let Some(workspace) = workspace {
                        let git_folder =
                            ignore::overrides::OverrideBuilder::new(&workspace)
                                .add("!.git/")
                                .map(|git_folder| git_folder.build());

                        let walker = match git_folder {
                            Ok(Ok(git_folder)) => {
                                ignore::WalkBuilder::new(&workspace)
                                    .hidden(false)
                                    .parents(false)
                                    .require_git(false)
                                    .overrides(git_folder)
                                    .build()
                            }
                            _ => ignore::WalkBuilder::new(&workspace)
                                .parents(false)
                                .require_git(false)
                                .build(),
                        };

                        let mut items = Vec::new();
                        for path in walker.flatten() {
                            if let Some(file_type) = path.file_type() {
                                if file_type.is_file() {
                                    items.push(path.into_path());
                                }
                            }
                        }
                        Ok(ProxyResponse::GetFilesResponse { items })
                    } else {
                        Ok(ProxyResponse::GetFilesResponse { items: Vec::new() })
                    };
                    proxy_rpc.handle_response(id, result);
                });
            }
            GetOpenFilesContent {} => {
                let items = self
                    .buffers
                    .iter()
                    .map(|(path, buffer)| TextDocumentItem {
                        uri: Url::from_file_path(path).unwrap(),
                        language_id: buffer.language_id.to_string(),
                        version: buffer.rev as i32,
                        text: buffer.get_document(),
                    })
                    .collect();
                let resp = ProxyResponse::GetOpenFilesContentResponse { items };
                self.proxy_rpc.handle_response(id, Ok(resp));
            }
            ReadDir { path } => {
                let proxy_rpc = self.proxy_rpc.clone();
                thread::spawn(move || {
                    let result = fs::read_dir(path)
                        .map(|entries| {
                            let mut items = entries
                                .into_iter()
                                .filter_map(|entry| {
                                    entry
                                        .map(|e| FileNodeItem {
                                            path: e.path(),
                                            is_dir: e.path().is_dir(),
                                            open: false,
                                            read: false,
                                            children: HashMap::new(),
                                            children_open_count: 0,
                                        })
                                        .ok()
                                })
                                .collect::<Vec<FileNodeItem>>();

                            items.sort();

                            ProxyResponse::ReadDirResponse { items }
                        })
                        .map_err(|e| RpcError {
                            code: 0,
                            message: e.to_string(),
                        });
                    proxy_rpc.handle_response(id, result);
                });
            }
            Save {
                rev,
                path,
                create_parents,
            } => {
                let buffer = self.buffers.get_mut(&path).unwrap();
                let result = buffer
                    .save(rev, create_parents)
                    .map(|_r| {
                        self.catalog_rpc
                            .did_save_text_document(&path, buffer.rope.clone());
                        ProxyResponse::SaveResponse {}
                    })
                    .map_err(|e| RpcError {
                        code: 0,
                        message: e.to_string(),
                    });
                self.respond_rpc(id, result);
            }
            SaveBufferAs {
                buffer_id,
                path,
                rev,
                content,
                create_parents,
            } => {
                let mut buffer = Buffer::new(buffer_id, path.clone());
                buffer.rope = Rope::from(content);
                buffer.rev = rev;
                let result = buffer
                    .save(rev, create_parents)
                    .map(|_| ProxyResponse::Success {})
                    .map_err(|e| RpcError {
                        code: 0,
                        message: e.to_string(),
                    });
                self.buffers.insert(path, buffer);
                self.respond_rpc(id, result);
            }
            CreateFile { path } => {
                let result = path
                    .parent()
                    .map_or(Ok(()), std::fs::create_dir_all)
                    .and_then(|()| {
                        std::fs::OpenOptions::new()
                            .write(true)
                            .create_new(true)
                            .open(path)
                    })
                    .map(|_| ProxyResponse::Success {})
                    .map_err(|e| RpcError {
                        code: 0,
                        message: e.to_string(),
                    });
                self.respond_rpc(id, result);
            }
            CreateDirectory { path } => {
                let result = std::fs::create_dir_all(path)
                    .map(|_| ProxyResponse::Success {})
                    .map_err(|e| RpcError {
                        code: 0,
                        message: e.to_string(),
                    });
                self.respond_rpc(id, result);
            }
            TrashPath { path } => {
                let result = trash::delete(path)
                    .map(|_| ProxyResponse::Success {})
                    .map_err(|e| RpcError {
                        code: 0,
                        message: e.to_string(),
                    });
                self.respond_rpc(id, result);
            }
            DuplicatePath {
                existing_path,
                new_path,
            } => {
                // We first check if the destination already exists, because copy can overwrite it
                // and that's not the default behavior we want for when a user duplicates a document.
                let result = if new_path.exists() {
                    Err(RpcError {
                        code: 0,
                        message: format!("{new_path:?} already exists"),
                    })
                } else {
                    if let Some(parent) = new_path.parent() {
                        if let Err(error) = std::fs::create_dir_all(parent) {
                            let result = Err(RpcError {
                                code: 0,
                                message: error.to_string(),
                            });
                            self.respond_rpc(id, result);
                            return;
                        }
                    }
                    std::fs::copy(existing_path, new_path)
                        .map(|_| ProxyResponse::Success {})
                        .map_err(|e| RpcError {
                            code: 0,
                            message: e.to_string(),
                        })
                };
                self.respond_rpc(id, result);
            }
            RenamePath { from, to } => {
                // We first check if the destination already exists, because rename can overwrite it
                // and that's not the default behavior we want for when a user renames a document.
                let result = if to.exists() {
                    Err(format!("{} already exists", to.display()))
                } else {
                    Ok(())
                };

                let result = result.and_then(|_| {
                    if let Some(parent) = to.parent() {
                        fs::create_dir_all(parent).map_err(|e| {
                            if let io::ErrorKind::AlreadyExists = e.kind() {
                                format!(
                                    "{} has a parent that is not a directory",
                                    to.display()
                                )
                            } else {
                                e.to_string()
                            }
                        })
                    } else {
                        Ok(())
                    }
                });

                let result = result
                    .and_then(|_| fs::rename(&from, &to).map_err(|e| e.to_string()));

                let result = result
                    .map(|_| {
                        let to = to.canonicalize().unwrap_or(to);

                        let (is_dir, is_file) = to
                            .metadata()
                            .map(|metadata| (metadata.is_dir(), metadata.is_file()))
                            .unwrap_or((false, false));

                        if is_dir {
                            // Update all buffers in which a file the renamed directory is an
                            // ancestor of is open to use the file's new path.
                            // This could be written more nicely if `HashMap::extract_if` were
                            // stable.
                            let child_buffers: Vec<_> = self
                                .buffers
                                .keys()
                                .filter_map(|path| {
                                    path.strip_prefix(&from).ok().map(|suffix| {
                                        (path.clone(), suffix.to_owned())
                                    })
                                })
                                .collect();

                            for (path, suffix) in child_buffers {
                                if let Some(mut buffer) = self.buffers.remove(&path)
                                {
                                    let new_path = to.join(suffix);
                                    buffer.path = new_path;

                                    self.buffers.insert(buffer.path.clone(), buffer);
                                }
                            }
                        } else if is_file {
                            // If the renamed file is open in a buffer, update it to use the new
                            // path.
                            let buffer = self.buffers.remove(&from);

                            if let Some(mut buffer) = buffer {
                                buffer.path.clone_from(&to);
                                self.buffers.insert(to.clone(), buffer);
                            }
                        }

                        ProxyResponse::CreatePathResponse { path: to }
                    })
                    .map_err(|message| RpcError { code: 0, message });

                self.respond_rpc(id, result);
            }
            TestCreateAtPath { path } => {
                // This performs a best effort test to see if an attempt to create an item at
                // `path` or rename an item to `path` will succeed.
                // Currently the only conditions that are tested are that `path` doesn't already
                // exist and that `path` doesn't have a parent that exists and is not a directory.
                let result = if path.exists() {
                    Err(format!("{} already exists", path.display()))
                } else {
                    Ok(path)
                };

                let result = result
                    .and_then(|path| {
                        let parent_is_dir = path
                            .ancestors()
                            .skip(1)
                            .find(|parent| parent.exists())
                            .is_none_or(|parent| parent.is_dir());

                        if parent_is_dir {
                            Ok(ProxyResponse::Success {})
                        } else {
                            Err(format!(
                                "{} has a parent that is not a directory",
                                path.display()
                            ))
                        }
                    })
                    .map_err(|message| RpcError { code: 0, message });

                self.respond_rpc(id, result);
            }
            GetSelectionRange { positions, path } => {
                let proxy_rpc = self.proxy_rpc.clone();
                self.catalog_rpc.get_selection_range(
                    path.as_path(),
                    positions,
                    move |_, result| {
                        let result = result.map(|ranges| {
                            ProxyResponse::GetSelectionRange { ranges }
                        });
                        proxy_rpc.handle_response(id, result);
                    },
                );
            }
            CodeActionResolve {
                action_item,
                plugin_id,
            } => {
                let proxy_rpc = self.proxy_rpc.clone();
                self.catalog_rpc.action_resolve(
                    *action_item,
                    plugin_id,
                    move |result| {
                        let result = result.map(|item| {
                            ProxyResponse::CodeActionResolveResponse {
                                item: Box::new(item),
                            }
                        });
                        proxy_rpc.handle_response(id, result);
                    },
                );
            }
            DapVariable { dap_id, reference } => {
                let proxy_rpc = self.proxy_rpc.clone();
                self.catalog_rpc
                    .dap_variable(dap_id, reference, move |result| {
                        proxy_rpc.handle_response(
                            id,
                            result.map(|resp| ProxyResponse::DapVariableResponse {
                                varialbes: resp,
                            }),
                        );
                    });
            }
            DapGetScopes { dap_id, frame_id } => {
                let proxy_rpc = self.proxy_rpc.clone();
                self.catalog_rpc
                    .dap_get_scopes(dap_id, frame_id, move |result| {
                        proxy_rpc.handle_response(
                            id,
                            result.map(|resp| ProxyResponse::DapGetScopesResponse {
                                scopes: resp,
                            }),
                        );
                    });
            }
            GetCodeLens { path } => {
                let proxy_rpc = self.proxy_rpc.clone();
                self.catalog_rpc
                    .get_code_lens(&path, move |plugin_id, result| {
                        let result = result.map(|resp| {
                            ProxyResponse::GetCodeLensResponse { plugin_id, resp }
                        });
                        proxy_rpc.handle_response(id, result);
                    });
            }
            LspFoldingRange { path } => {
                let proxy_rpc = self.proxy_rpc.clone();
                self.catalog_rpc.get_lsp_folding_range(
                    &path,
                    move |plugin_id, result| {
                        let result = result.map(|resp| {
                            ProxyResponse::LspFoldingRangeResponse {
                                plugin_id,
                                resp,
                            }
                        });
                        proxy_rpc.handle_response(id, result);
                    },
                );
            }
            GetCodeLensResolve { code_lens, path } => {
                let proxy_rpc = self.proxy_rpc.clone();
                self.catalog_rpc.get_code_lens_resolve(
                    &path,
                    &code_lens,
                    move |plugin_id, result| {
                        let result = result.map(|resp| {
                            ProxyResponse::GetCodeLensResolveResponse {
                                plugin_id,
                                resp,
                            }
                        });
                        proxy_rpc.handle_response(id, result);
                    },
                );
            }
            GotoImplementation { path, position } => {
                let proxy_rpc = self.proxy_rpc.clone();
                self.catalog_rpc.go_to_implementation(
                    &path,
                    position,
                    move |plugin_id, result| {
                        let result = result.map(|resp| {
                            ProxyResponse::GotoImplementationResponse {
                                plugin_id,
                                resp,
                            }
                        });
                        proxy_rpc.handle_response(id, result);
                    },
                );
            }
            ReferencesResolve { items } => {
                let items: Vec<FileLine> = items
                    .into_iter()
                    .filter_map(|location| {
                        let Ok(path) = location.uri.to_file_path() else {
                            tracing::error!(
                                "get file path fail: {:?}",
                                location.uri
                            );
                            return None;
                        };
                        let buffer = self.get_buffer_or_insert(path.clone());
                        let line_num = location.range.start.line as usize;
                        let content = buffer.line_to_cow(line_num).to_string();
                        Some(FileLine {
                            path,
                            position: location.range.start,
                            content,
                        })
                    })
                    .collect();
                let resp = ProxyResponse::ReferencesResolveResponse { items };
                self.proxy_rpc.handle_response(id, Ok(resp));
            }
        }
    }
}

impl Dispatcher {
    pub fn new(core_rpc: CoreRpcHandler, proxy_rpc: ProxyRpcHandler) -> Self {
        let plugin_rpc =
            PluginCatalogRpcHandler::new(core_rpc.clone(), proxy_rpc.clone());

        let file_watcher = FileWatcher::new();

        Self {
            workspace: None,
            proxy_rpc,
            core_rpc,
            catalog_rpc: plugin_rpc,
            buffers: HashMap::new(),
            terminals: HashMap::new(),
            file_watcher,
            window_id: 1,
            tab_id: 1,
        }
    }

    fn respond_rpc(&self, id: RequestId, result: Result<ProxyResponse, RpcError>) {
        self.proxy_rpc.handle_response(id, result);
    }

    fn get_buffer_or_insert(&mut self, path: PathBuf) -> &mut Buffer {
        self.buffers
            .entry(path.clone())
            .or_insert(Buffer::new(BufferId::next(), path))
    }
}

struct FileWatchNotifier {
    core_rpc: CoreRpcHandler,
    proxy_rpc: ProxyRpcHandler,
    workspace: Option<PathBuf>,
    workspace_fs_change_handler: Arc<Mutex<Option<Sender<bool>>>>,
    last_diff: Arc<Mutex<DiffInfo>>,
}

impl Notify for FileWatchNotifier {
    fn notify(&self, events: Vec<(WatchToken, notify::Event)>) {
        self.handle_fs_events(events);
    }
}

impl FileWatchNotifier {
    fn new(
        workspace: Option<PathBuf>,
        core_rpc: CoreRpcHandler,
        proxy_rpc: ProxyRpcHandler,
    ) -> Self {
        let notifier = Self {
            workspace,
            core_rpc,
            proxy_rpc,
            workspace_fs_change_handler: Arc::new(Mutex::new(None)),
            last_diff: Arc::new(Mutex::new(DiffInfo::default())),
        };

        if let Some(workspace) = notifier.workspace.clone() {
            let core_rpc = notifier.core_rpc.clone();
            let last_diff = notifier.last_diff.clone();
            thread::spawn(move || {
                if let Some(diff) = git_diff_new(&workspace) {
                    core_rpc.diff_info(diff.clone());
                    *last_diff.lock() = diff;
                }
            });
        }

        notifier
    }

    fn handle_fs_events(&self, events: Vec<(WatchToken, notify::Event)>) {
        for (token, event) in events {
            match token {
                OPEN_FILE_EVENT_TOKEN => self.handle_open_file_fs_event(event),
                WORKSPACE_EVENT_TOKEN => self.handle_workspace_fs_event(event),
                _ => {}
            }
        }
    }

    fn handle_open_file_fs_event(&self, event: notify::Event) {
        if event.kind.is_modify() || event.kind.is_remove() {
            for path in event.paths {
                #[cfg(windows)]
                if let Some(path_str) = path.to_str() {
                    const PREFIX: &str = r"\\?\";
                    if let Some(path_str) = path_str.strip_prefix(PREFIX) {
                        let path = PathBuf::from(&path_str);
                        self.proxy_rpc.notification(
                            ProxyNotification::OpenFileChanged { path },
                        );
                        continue;
                    }
                }
                self.proxy_rpc
                    .notification(ProxyNotification::OpenFileChanged { path });
            }
        }
    }

    fn handle_workspace_fs_event(&self, event: notify::Event) {
        let explorer_change = match &event.kind {
            notify::EventKind::Create(_)
            | notify::EventKind::Remove(_)
            | notify::EventKind::Modify(notify::event::ModifyKind::Name(_)) => true,
            notify::EventKind::Modify(_) => false,
            _ => return,
        };

        let mut handler = self.workspace_fs_change_handler.lock();
        if let Some(sender) = handler.as_mut() {
            if explorer_change {
                // only send the value if we need to update file explorer as well
                if let Err(err) = sender.send(explorer_change) {
                    tracing::error!("{:?}", err);
                }
            }
            return;
        }
        let (sender, receiver) = crossbeam_channel::unbounded();
        if explorer_change {
            // only send the value if we need to update file explorer as well
            if let Err(err) = sender.send(explorer_change) {
                tracing::error!("{:?}", err);
            }
        }

        let local_handler = self.workspace_fs_change_handler.clone();
        let core_rpc = self.core_rpc.clone();
        let workspace = self.workspace.clone().unwrap();
        let last_diff = self.last_diff.clone();
        thread::spawn(move || {
            thread::sleep(Duration::from_millis(500));

            {
                local_handler.lock().take();
            }

            let mut explorer_change = false;
            for e in receiver {
                if e {
                    explorer_change = true;
                    break;
                }
            }
            if explorer_change {
                core_rpc.workspace_file_change();
            }
            if let Some(diff) = git_diff_new(&workspace) {
                let mut last_diff = last_diff.lock();
                if diff != *last_diff {
                    core_rpc.diff_info(diff.clone());
                    *last_diff = diff;
                }
            }
        });
        *handler = Some(sender);
    }
}

#[derive(Clone, Debug)]
pub struct DiffHunk {
    pub old_start: u32,
    pub old_lines: u32,
    pub new_start: u32,
    pub new_lines: u32,
    pub header: String,
}

fn git_init(workspace_path: &Path) -> Result<()> {
    if Repository::discover(workspace_path).is_err() {
        Repository::init(workspace_path)?;
    };
    Ok(())
}

fn git_commit(
    workspace_path: &Path,
    message: &str,
    diffs: Vec<FileDiff>,
) -> Result<()> {
    let repo = Repository::discover(workspace_path)?;
    let mut index = repo.index()?;
    for diff in diffs {
        match diff {
            FileDiff::Modified(p) | FileDiff::Added(p) => {
                index.add_path(p.strip_prefix(workspace_path)?)?;
            }
            FileDiff::Renamed(a, d) => {
                index.add_path(a.strip_prefix(workspace_path)?)?;
                index.remove_path(d.strip_prefix(workspace_path)?)?;
            }
            FileDiff::Deleted(p) => {
                index.remove_path(p.strip_prefix(workspace_path)?)?;
            }
        }
    }
    index.write()?;
    let tree = index.write_tree()?;
    let tree = repo.find_tree(tree)?;

    match repo.signature() {
        Ok(signature) => {
            let parents = repo
                .head()
                .and_then(|head| Ok(vec![head.peel_to_commit()?]))
                .unwrap_or(vec![]);
            let parents_refs = parents.iter().collect::<Vec<_>>();

            repo.commit(
                Some("HEAD"),
                &signature,
                &signature,
                message,
                &tree,
                &parents_refs,
            )?;
            Ok(())
        }
        Err(e) => match e.code() {
            NotFound => Err(anyhow!(
                "No user.name and/or user.email configured for this git repository."
            )),
            _ => Err(anyhow!(
                "Error while creating commit's signature: {}",
                e.message()
            )),
        },
    }
}

fn git_checkout(workspace_path: &Path, reference: &str) -> Result<()> {
    let repo = Repository::discover(workspace_path)?;
    let (object, reference) = repo.revparse_ext(reference)?;
    repo.checkout_tree(&object, None)?;
    repo.set_head(reference.unwrap().name().unwrap())?;
    Ok(())
}

fn git_discard_files_changes<'a>(
    workspace_path: &Path,
    files: impl Iterator<Item = &'a Path>,
) -> Result<()> {
    let repo = Repository::discover(workspace_path)?;

    let mut checkout_b = CheckoutBuilder::new();
    checkout_b.update_only(false).force();

    let mut had_path = false;
    for path in files {
        // Remove the workspace path so it is relative to the folder
        if let Ok(path) = path.strip_prefix(workspace_path) {
            had_path = true;
            checkout_b.path(path);
        }
    }

    if !had_path {
        // If there we no paths then we do nothing
        // because the default behavior of checkout builder is to select all files
        // if it is not given a path
        return Ok(());
    }

    repo.checkout_index(None, Some(&mut checkout_b))?;

    Ok(())
}

fn git_discard_workspace_changes(workspace_path: &Path) -> Result<()> {
    let repo = Repository::discover(workspace_path)?;
    let mut checkout_b = CheckoutBuilder::new();
    checkout_b.force();

    repo.checkout_index(None, Some(&mut checkout_b))?;

    Ok(())
}

fn git_delta_format(
    workspace_path: &Path,
    delta: &git2::DiffDelta,
) -> Option<(git2::Delta, git2::Oid, PathBuf)> {
    match delta.status() {
        git2::Delta::Added | git2::Delta::Untracked => Some((
            git2::Delta::Added,
            delta.new_file().id(),
            delta.new_file().path().map(|p| workspace_path.join(p))?,
        )),
        git2::Delta::Deleted => Some((
            git2::Delta::Deleted,
            delta.old_file().id(),
            delta.old_file().path().map(|p| workspace_path.join(p))?,
        )),
        git2::Delta::Modified => Some((
            git2::Delta::Modified,
            delta.new_file().id(),
            delta.new_file().path().map(|p| workspace_path.join(p))?,
        )),
        _ => None,
    }
}

fn git_diff_new(workspace_path: &Path) -> Option<DiffInfo> {
    let repo = Repository::discover(workspace_path).ok()?;
    let name = match repo.head() {
        Ok(head) => head.shorthand()?.to_string(),
        _ => "(No branch)".to_owned(),
    };

    let mut branches = Vec::new();
    for branch in repo.branches(None).ok()? {
        branches.push(branch.ok()?.0.name().ok()??.to_string());
    }

    let mut tags = Vec::new();
    if let Ok(git_tags) = repo.tag_names(None) {
        for tag in git_tags.into_iter().flatten() {
            tags.push(tag.to_owned());
        }
    }

    let mut deltas = Vec::new();
    let mut diff_options = DiffOptions::new();
    let diff = repo
        .diff_index_to_workdir(
            None,
            Some(
                diff_options
                    .include_untracked(true)
                    .recurse_untracked_dirs(true),
            ),
        )
        .ok()?;
    for delta in diff.deltas() {
        if let Some(delta) = git_delta_format(workspace_path, &delta) {
            deltas.push(delta);
        }
    }

    let oid = match repo.revparse_single("HEAD^{tree}") {
        Ok(obj) => obj.id(),
        _ => Oid::zero(),
    };

    let cached_diff = repo
        .diff_tree_to_index(repo.find_tree(oid).ok().as_ref(), None, None)
        .ok();

    if let Some(cached_diff) = cached_diff {
        for delta in cached_diff.deltas() {
            if let Some(delta) = git_delta_format(workspace_path, &delta) {
                deltas.push(delta);
            }
        }
    }
    let mut renames = Vec::new();
    let mut renamed_deltas = HashSet::new();

    for (added_index, delta) in deltas.iter().enumerate() {
        if delta.0 == git2::Delta::Added {
            for (deleted_index, d) in deltas.iter().enumerate() {
                if d.0 == git2::Delta::Deleted && d.1 == delta.1 {
                    renames.push((added_index, deleted_index));
                    renamed_deltas.insert(added_index);
                    renamed_deltas.insert(deleted_index);
                    break;
                }
            }
        }
    }

    let mut file_diffs = Vec::new();
    for (added_index, deleted_index) in renames.iter() {
        file_diffs.push(FileDiff::Renamed(
            deltas[*added_index].2.clone(),
            deltas[*deleted_index].2.clone(),
        ));
    }
    for (i, delta) in deltas.iter().enumerate() {
        if renamed_deltas.contains(&i) {
            continue;
        }
        let diff = match delta.0 {
            git2::Delta::Added => FileDiff::Added(delta.2.clone()),
            git2::Delta::Deleted => FileDiff::Deleted(delta.2.clone()),
            git2::Delta::Modified => FileDiff::Modified(delta.2.clone()),
            _ => continue,
        };
        file_diffs.push(diff);
    }
    file_diffs.sort_by_key(|d| match d {
        FileDiff::Modified(p)
        | FileDiff::Added(p)
        | FileDiff::Renamed(p, _)
        | FileDiff::Deleted(p) => p.clone(),
    });
    Some(DiffInfo {
        head: name,
        branches,
        tags,
        diffs: file_diffs,
    })
}

/// Get git commit log with optional filters
fn git_log(
    workspace_path: &Path,
    limit: usize,
    skip: usize,
    branch: Option<String>,
    author: Option<String>,
    search: Option<String>,
) -> Result<lapce_rpc::source_control::GitLogResult> {
    use lapce_rpc::source_control::{GitCommitInfo, GitLogResult};
    
    let repo = Repository::discover(workspace_path)?;
    
    // Get HEAD commit id for checking if commit is HEAD
    let head_id = repo.head().ok().and_then(|h| h.target());
    
    // Get all branches for annotation
    let mut branch_map: std::collections::HashMap<git2::Oid, Vec<String>> = std::collections::HashMap::new();
    if let Ok(branches) = repo.branches(None) {
        for branch_result in branches {
            if let Ok((branch, _)) = branch_result {
                if let (Some(name), Some(target)) = (branch.name().ok().flatten(), branch.get().target()) {
                    branch_map.entry(target).or_default().push(name.to_string());
                }
            }
        }
    }
    
    // Get all tags for annotation
    let mut tag_map: std::collections::HashMap<git2::Oid, Vec<String>> = std::collections::HashMap::new();
    if let Ok(tags) = repo.tag_names(None) {
        for tag_name in tags.iter().flatten() {
            if let Ok(obj) = repo.revparse_single(&format!("refs/tags/{}", tag_name)) {
                // For annotated tags, peel to the commit
                let commit_id = obj.peel_to_commit().map(|c| c.id()).unwrap_or(obj.id());
                tag_map.entry(commit_id).or_default().push(tag_name.to_string());
            }
        }
    }
    
    // Set up revwalk
    let mut revwalk = repo.revwalk()?;
    revwalk.set_sorting(git2::Sort::TIME | git2::Sort::TOPOLOGICAL)?;
    
    // Start from branch or HEAD
    if let Some(ref branch_name) = branch {
        if let Ok(reference) = repo.resolve_reference_from_short_name(branch_name) {
            if let Some(oid) = reference.target() {
                revwalk.push(oid)?;
            }
        } else {
            revwalk.push_head()?;
        }
    } else {
        revwalk.push_head()?;
    }
    
    let mut commits = Vec::new();
    let mut total_count = 0;
    
    for oid_result in revwalk {
        let oid = match oid_result {
            Ok(oid) => oid,
            Err(_) => continue,
        };
        
        let commit = match repo.find_commit(oid) {
            Ok(c) => c,
            Err(_) => continue,
        };
        
        // Apply author filter
        if let Some(ref author_filter) = author {
            let commit_author = commit.author();
            let author_name = commit_author.name().unwrap_or("");
            let author_email = commit_author.email().unwrap_or("");
            if !author_name.to_lowercase().contains(&author_filter.to_lowercase())
                && !author_email.to_lowercase().contains(&author_filter.to_lowercase())
            {
                continue;
            }
        }
        
        // Apply search filter (commit message)
        if let Some(ref search_text) = search {
            let message = commit.message().unwrap_or("");
            if !message.to_lowercase().contains(&search_text.to_lowercase()) {
                continue;
            }
        }
        
        total_count += 1;
        
        // Apply skip and limit
        if total_count <= skip {
            continue;
        }
        if commits.len() >= limit {
            continue; // Keep counting for total_count
        }
        
        let author_sig = commit.author();
        let parents: Vec<String> = commit.parent_ids().map(|id| id.to_string()).collect();
        
        let commit_info = GitCommitInfo {
            id: oid.to_string(),
            short_id: oid.to_string().chars().take(7).collect(),
            summary: commit.summary().unwrap_or("").to_string(),
            message: commit.message().unwrap_or("").to_string(),
            author_name: author_sig.name().unwrap_or("").to_string(),
            author_email: author_sig.email().unwrap_or("").to_string(),
            timestamp: commit.time().seconds(),
            parents,
            branches: branch_map.get(&oid).cloned().unwrap_or_default(),
            tags: tag_map.get(&oid).cloned().unwrap_or_default(),
            is_head: head_id == Some(oid),
        };
        
        commits.push(commit_info);
    }
    
    Ok(GitLogResult {
        commits,
        total_count,
    })
}

// ============================================================================
// Git Branch Operations
// ============================================================================

fn git_list_branches(
    workspace_path: &Path,
    include_remote: bool,
) -> Result<Vec<lapce_rpc::source_control::GitBranchInfo>> {
    use lapce_rpc::source_control::GitBranchInfo;
    
    let repo = Repository::discover(workspace_path)?;
    let head = repo.head().ok();
    let head_name = head.as_ref().and_then(|h| h.shorthand().map(|s| s.to_string()));
    
    let filter = if include_remote {
        None
    } else {
        Some(git2::BranchType::Local)
    };
    
    let mut branches = Vec::new();
    for branch_result in repo.branches(filter)? {
        let (branch, branch_type) = branch_result?;
        let name = branch.name()?.unwrap_or("").to_string();
        let is_remote = branch_type == git2::BranchType::Remote;
        let is_head = head_name.as_ref() == Some(&name);
        
        // Get upstream info
        let (upstream, ahead, behind) = if !is_remote {
            if let Ok(upstream_branch) = branch.upstream() {
                let upstream_name = upstream_branch.name().ok().flatten().map(|s| s.to_string());
                let (ahead, behind) = if let (Some(local_oid), Some(upstream_oid)) = 
                    (branch.get().target(), upstream_branch.get().target()) 
                {
                    repo.graph_ahead_behind(local_oid, upstream_oid).unwrap_or((0, 0))
                } else {
                    (0, 0)
                };
                (upstream_name, ahead, behind)
            } else {
                (None, 0, 0)
            }
        } else {
            (None, 0, 0)
        };
        
        // Get last commit info
        let (last_commit_id, last_commit_summary) = if let Some(oid) = branch.get().target() {
            if let Ok(commit) = repo.find_commit(oid) {
                (Some(oid.to_string()), commit.summary().map(|s| s.to_string()))
            } else {
                (None, None)
            }
        } else {
            (None, None)
        };
        
        branches.push(GitBranchInfo {
            name,
            is_remote,
            is_head,
            upstream,
            ahead,
            behind,
            last_commit_id,
            last_commit_summary,
        });
    }
    
    Ok(branches)
}

fn git_create_branch(
    workspace_path: &Path,
    name: &str,
    start_point: Option<&str>,
    checkout: bool,
) -> Result<lapce_rpc::source_control::GitBranchResult> {
    use lapce_rpc::source_control::GitBranchResult;
    
    let repo = Repository::discover(workspace_path)?;
    
    // Get the commit to branch from
    let commit = if let Some(start) = start_point {
        let obj = repo.revparse_single(start)?;
        obj.peel_to_commit()?
    } else {
        repo.head()?.peel_to_commit()?
    };
    
    // Create the branch
    repo.branch(name, &commit, false)?;
    
    // Checkout if requested
    if checkout {
        let refname = format!("refs/heads/{}", name);
        let obj = repo.revparse_single(&refname)?;
        repo.checkout_tree(&obj, None)?;
        repo.set_head(&refname)?;
    }
    
    Ok(GitBranchResult {
        success: true,
        message: format!("Created branch '{}'", name),
        branch_name: Some(name.to_string()),
    })
}

fn git_delete_branch(
    workspace_path: &Path,
    name: &str,
    force: bool,
    _delete_remote: bool,
) -> Result<lapce_rpc::source_control::GitBranchResult> {
    use lapce_rpc::source_control::GitBranchResult;
    
    let repo = Repository::discover(workspace_path)?;
    
    let mut branch = repo.find_branch(name, git2::BranchType::Local)?;
    
    if force {
        branch.delete()?;
    } else {
        // Check if fully merged
        if !branch.is_head() {
            branch.delete()?;
        } else {
            return Err(anyhow!("Cannot delete the currently checked out branch"));
        }
    }
    
    Ok(GitBranchResult {
        success: true,
        message: format!("Deleted branch '{}'", name),
        branch_name: Some(name.to_string()),
    })
}

fn git_rename_branch(
    workspace_path: &Path,
    old_name: &str,
    new_name: &str,
) -> Result<lapce_rpc::source_control::GitBranchResult> {
    use lapce_rpc::source_control::GitBranchResult;
    
    let repo = Repository::discover(workspace_path)?;
    let mut branch = repo.find_branch(old_name, git2::BranchType::Local)?;
    branch.rename(new_name, false)?;
    
    Ok(GitBranchResult {
        success: true,
        message: format!("Renamed branch '{}' to '{}'", old_name, new_name),
        branch_name: Some(new_name.to_string()),
    })
}

// ============================================================================
// Git Stash Operations
// ============================================================================

fn git_stash_list(workspace_path: &Path) -> Result<lapce_rpc::source_control::GitStashList> {
    use lapce_rpc::source_control::{GitStashEntry, GitStashList};
    
    let repo = Repository::discover(workspace_path)?;
    let mut entries = Vec::new();
    let mut stash_data: Vec<(usize, String, git2::Oid)> = Vec::new();
    
    // First collect stash data without borrowing repo
    {
        let mut repo_mut = repo;
        repo_mut.stash_foreach(|index, message, oid| {
            stash_data.push((index, message.to_string(), *oid));
            true
        })?;
        
        // Now process the collected data
        for (index, message, oid) in stash_data {
            let commit = repo_mut.find_commit(oid).ok();
            let (branch, timestamp) = if let Some(ref c) = commit {
                let branch = c.summary().unwrap_or("").to_string();
                let timestamp = c.time().seconds();
                (branch, timestamp)
            } else {
                (String::new(), 0)
            };
            
            entries.push(GitStashEntry {
                index,
                message,
                branch,
                commit_id: oid.to_string(),
                timestamp,
            });
        }
    }
    
    Ok(GitStashList { entries })
}

fn git_stash_save(
    workspace_path: &Path,
    message: Option<&str>,
    include_untracked: bool,
    keep_index: bool,
) -> Result<lapce_rpc::source_control::GitStashResult> {
    use lapce_rpc::source_control::GitStashResult;
    
    let repo = Repository::discover(workspace_path)?;
    let sig = repo.signature()?;
    
    let mut flags = git2::StashFlags::DEFAULT;
    if include_untracked {
        flags |= git2::StashFlags::INCLUDE_UNTRACKED;
    }
    if keep_index {
        flags |= git2::StashFlags::KEEP_INDEX;
    }
    
    // Need mutable repo for stash_save
    let mut repo = repo;
    let _oid = repo.stash_save(&sig, message.unwrap_or("WIP"), Some(flags))?;
    
    Ok(GitStashResult {
        success: true,
        message: "Stash created".to_string(),
    })
}

fn git_stash_pop(workspace_path: &Path, index: usize) -> Result<lapce_rpc::source_control::GitStashResult> {
    use lapce_rpc::source_control::GitStashResult;
    
    let repo = Repository::discover(workspace_path)?;
    let mut repo = repo;
    repo.stash_pop(index, None)?;
    
    Ok(GitStashResult {
        success: true,
        message: "Stash applied and dropped".to_string(),
    })
}

fn git_stash_apply(workspace_path: &Path, index: usize) -> Result<lapce_rpc::source_control::GitStashResult> {
    use lapce_rpc::source_control::GitStashResult;
    
    let mut repo = Repository::discover(workspace_path)?;
    repo.stash_apply(index, None)?;
    
    Ok(GitStashResult {
        success: true,
        message: "Stash applied".to_string(),
    })
}

fn git_stash_drop(workspace_path: &Path, index: usize) -> Result<lapce_rpc::source_control::GitStashResult> {
    use lapce_rpc::source_control::GitStashResult;
    
    let repo = Repository::discover(workspace_path)?;
    let mut repo = repo;
    repo.stash_drop(index)?;
    
    Ok(GitStashResult {
        success: true,
        message: "Stash dropped".to_string(),
    })
}

// ============================================================================
// Git Merge Operations
// ============================================================================

fn git_merge(
    workspace_path: &Path,
    options: &lapce_rpc::source_control::GitMergeOptions,
) -> Result<lapce_rpc::source_control::GitMergeResult> {
    use lapce_rpc::source_control::GitMergeResult;
    
    let repo = Repository::discover(workspace_path)?;
    
    // Find the branch/commit to merge
    let reference = repo.resolve_reference_from_short_name(&options.branch)?;
    let annotated_commit = repo.reference_to_annotated_commit(&reference)?;
    
    // Perform merge analysis
    let (analysis, _preference) = repo.merge_analysis(&[&annotated_commit])?;
    
    if analysis.is_up_to_date() {
        return Ok(GitMergeResult {
            success: true,
            message: "Already up to date".to_string(),
            conflicts: Vec::new(),
            merged_commit: None,
        });
    }
    
    if analysis.is_fast_forward() && !options.no_ff {
        // Fast-forward merge
        let refname = format!("refs/heads/{}", repo.head()?.shorthand().unwrap_or("HEAD"));
        let target_oid = annotated_commit.id();
        let mut reference = repo.find_reference(&refname)?;
        reference.set_target(target_oid, &format!("Fast-forward to {}", options.branch))?;
        repo.checkout_head(Some(git2::build::CheckoutBuilder::new().force()))?;
        
        return Ok(GitMergeResult {
            success: true,
            message: format!("Fast-forwarded to {}", options.branch),
            conflicts: Vec::new(),
            merged_commit: Some(target_oid.to_string()),
        });
    }
    
    // Regular merge
    repo.merge(&[&annotated_commit], None, None)?;
    
    // Check for conflicts
    let mut index = repo.index()?;
    if index.has_conflicts() {
        let conflicts: Vec<PathBuf> = index.conflicts()?
            .filter_map(|c| c.ok())
            .filter_map(|c| c.our.or(c.their).map(|e| PathBuf::from(String::from_utf8_lossy(&e.path).to_string())))
            .collect();
        
        return Ok(GitMergeResult {
            success: false,
            message: "Merge has conflicts".to_string(),
            conflicts,
            merged_commit: None,
        });
    }
    
    // Create merge commit if not squashing
    if !options.squash {
        let sig = repo.signature()?;
        let message = options.message.clone().unwrap_or_else(|| 
            format!("Merge branch '{}'", options.branch)
        );
        let head_commit = repo.head()?.peel_to_commit()?;
        let merge_commit = repo.find_commit(annotated_commit.id())?;
        let tree_oid = index.write_tree()?;
        let tree = repo.find_tree(tree_oid)?;
        
        let oid = repo.commit(
            Some("HEAD"),
            &sig,
            &sig,
            &message,
            &tree,
            &[&head_commit, &merge_commit],
        )?;
        
        repo.cleanup_state()?;
        
        return Ok(GitMergeResult {
            success: true,
            message: format!("Merged branch '{}'", options.branch),
            conflicts: Vec::new(),
            merged_commit: Some(oid.to_string()),
        });
    }
    
    Ok(GitMergeResult {
        success: true,
        message: "Merge completed (squash)".to_string(),
        conflicts: Vec::new(),
        merged_commit: None,
    })
}

fn git_merge_abort(workspace_path: &Path) -> Result<lapce_rpc::source_control::GitMergeResult> {
    use lapce_rpc::source_control::GitMergeResult;
    
    let repo = Repository::discover(workspace_path)?;
    repo.cleanup_state()?;
    
    // Reset to HEAD
    let head = repo.head()?.peel_to_commit()?;
    repo.reset(head.as_object(), git2::ResetType::Hard, None)?;
    
    Ok(GitMergeResult {
        success: true,
        message: "Merge aborted".to_string(),
        conflicts: Vec::new(),
        merged_commit: None,
    })
}

// ============================================================================
// Git Reset Operations
// ============================================================================

fn git_reset(
    workspace_path: &Path,
    options: &lapce_rpc::source_control::GitResetOptions,
) -> Result<lapce_rpc::source_control::GitResetResult> {
    use lapce_rpc::source_control::{GitResetMode, GitResetResult};
    
    let repo = Repository::discover(workspace_path)?;
    let obj = repo.revparse_single(&options.target)?;
    
    let reset_type = match options.mode {
        GitResetMode::Soft => git2::ResetType::Soft,
        GitResetMode::Mixed => git2::ResetType::Mixed,
        GitResetMode::Hard => git2::ResetType::Hard,
        GitResetMode::Keep => git2::ResetType::Mixed, // git2 doesn't have Keep
    };
    
    repo.reset(&obj, reset_type, None)?;
    
    let new_head = repo.head().ok().and_then(|h| h.target()).map(|o| o.to_string());
    
    Ok(GitResetResult {
        success: true,
        message: format!("Reset to {}", options.target),
        new_head,
    })
}

// ============================================================================
// Git Blame Operations
// ============================================================================

fn git_blame(
    workspace_path: &Path,
    path: &Path,
    commit: Option<&str>,
) -> Result<lapce_rpc::source_control::GitBlameResult> {
    use lapce_rpc::source_control::{GitBlameLine, GitBlameResult};
    
    let repo = Repository::discover(workspace_path)?;
    let relative_path = path.strip_prefix(workspace_path).unwrap_or(path);
    
    let mut opts = git2::BlameOptions::new();
    if let Some(commit_str) = commit {
        let oid = git2::Oid::from_str(commit_str)?;
        opts.newest_commit(oid);
    }
    
    let blame = repo.blame_file(relative_path, Some(&mut opts))?;
    
    let mut lines = Vec::new();
    for (line_no, hunk) in blame.iter().enumerate() {
        let commit_id = hunk.final_commit_id();
        let commit = repo.find_commit(commit_id).ok();
        
        let (author_name, author_email, timestamp, summary) = if let Some(ref c) = commit {
            let sig = c.author();
            (
                sig.name().unwrap_or("").to_string(),
                sig.email().unwrap_or("").to_string(),
                c.time().seconds(),
                c.summary().unwrap_or("").to_string(),
            )
        } else {
            (String::new(), String::new(), 0, String::new())
        };
        
        lines.push(GitBlameLine {
            line_number: line_no + 1,
            commit_id: commit_id.to_string(),
            short_commit_id: commit_id.to_string().chars().take(7).collect(),
            author_name,
            author_email,
            timestamp,
            summary,
            original_line_number: hunk.orig_start_line(),
            original_path: hunk.path().map(PathBuf::from),
        });
    }
    
    Ok(GitBlameResult {
        lines,
        path: path.to_path_buf(),
    })
}

// ============================================================================
// Git Tag Operations
// ============================================================================

fn git_list_tags(workspace_path: &Path) -> Result<Vec<lapce_rpc::source_control::GitTagInfo>> {
    use lapce_rpc::source_control::GitTagInfo;
    
    let repo = Repository::discover(workspace_path)?;
    let mut tags = Vec::new();
    
    for tag_name in repo.tag_names(None)?.iter().flatten() {
        let refname = format!("refs/tags/{}", tag_name);
        if let Ok(reference) = repo.find_reference(&refname) {
            let obj = reference.peel(git2::ObjectType::Any)?;
            
            let (commit_id, message, tagger_name, tagger_email, timestamp, is_annotated) = 
                if let Ok(tag) = obj.clone().into_tag() {
                    // Annotated tag
                    let target = tag.target()?.peel_to_commit()?;
                    let tagger = tag.tagger();
                    (
                        target.id().to_string(),
                        tag.message().map(|s| s.to_string()),
                        tagger.as_ref().and_then(|t| t.name()).map(|s| s.to_string()),
                        tagger.as_ref().and_then(|t| t.email()).map(|s| s.to_string()),
                        tagger.as_ref().map(|t| t.when().seconds()),
                        true,
                    )
                } else if let Ok(commit) = obj.peel_to_commit() {
                    // Lightweight tag
                    (commit.id().to_string(), None, None, None, None, false)
                } else {
                    continue;
                };
            
            tags.push(GitTagInfo {
                name: tag_name.to_string(),
                commit_id,
                message,
                tagger_name,
                tagger_email,
                timestamp,
                is_annotated,
            });
        }
    }
    
    Ok(tags)
}

fn git_create_tag(
    workspace_path: &Path,
    name: &str,
    target: Option<&str>,
    message: Option<&str>,
    annotated: bool,
) -> Result<lapce_rpc::source_control::GitTagResult> {
    use lapce_rpc::source_control::GitTagResult;
    
    let repo = Repository::discover(workspace_path)?;
    
    let target_obj = if let Some(t) = target {
        repo.revparse_single(t)?
    } else {
        repo.head()?.peel(git2::ObjectType::Commit)?
    };
    
    if annotated {
        let sig = repo.signature()?;
        repo.tag(name, &target_obj, &sig, message.unwrap_or(""), false)?;
    } else {
        repo.tag_lightweight(name, &target_obj, false)?;
    }
    
    Ok(GitTagResult {
        success: true,
        message: format!("Created tag '{}'", name),
        tag_name: Some(name.to_string()),
    })
}

fn git_delete_tag(
    workspace_path: &Path,
    name: &str,
) -> Result<lapce_rpc::source_control::GitTagResult> {
    use lapce_rpc::source_control::GitTagResult;
    
    let repo = Repository::discover(workspace_path)?;
    repo.tag_delete(name)?;
    
    Ok(GitTagResult {
        success: true,
        message: format!("Deleted tag '{}'", name),
        tag_name: Some(name.to_string()),
    })
}

// ============================================================================
// Git Remote Operations
// ============================================================================

fn git_list_remotes(workspace_path: &Path) -> Result<lapce_rpc::source_control::GitRemoteList> {
    use lapce_rpc::source_control::{GitRemoteInfo, GitRemoteList};
    
    let repo = Repository::discover(workspace_path)?;
    let mut remotes = Vec::new();
    
    for remote_name in repo.remotes()?.iter().flatten() {
        if let Ok(remote) = repo.find_remote(remote_name) {
            remotes.push(GitRemoteInfo {
                name: remote_name.to_string(),
                fetch_url: remote.url().unwrap_or("").to_string(),
                push_url: remote.pushurl().unwrap_or(remote.url().unwrap_or("")).to_string(),
            });
        }
    }
    
    Ok(GitRemoteList { remotes })
}

fn git_add_remote(
    workspace_path: &Path,
    name: &str,
    url: &str,
) -> Result<lapce_rpc::source_control::GitRemoteResult> {
    use lapce_rpc::source_control::GitRemoteResult;
    
    let repo = Repository::discover(workspace_path)?;
    repo.remote(name, url)?;
    
    Ok(GitRemoteResult {
        success: true,
        message: format!("Added remote '{}'", name),
        updated_refs: Vec::new(),
    })
}

fn git_remove_remote(
    workspace_path: &Path,
    name: &str,
) -> Result<lapce_rpc::source_control::GitRemoteResult> {
    use lapce_rpc::source_control::GitRemoteResult;
    
    let repo = Repository::discover(workspace_path)?;
    repo.remote_delete(name)?;
    
    Ok(GitRemoteResult {
        success: true,
        message: format!("Removed remote '{}'", name),
        updated_refs: Vec::new(),
    })
}

// ============================================================================
// Git Status Operations
// ============================================================================

fn git_get_status(workspace_path: &Path) -> Result<lapce_rpc::source_control::GitStatus> {
    use lapce_rpc::source_control::GitStatus;
    
    let repo = Repository::discover(workspace_path)?;
    let state = repo.state();
    
    let is_rebasing = matches!(state, 
        git2::RepositoryState::Rebase | 
        git2::RepositoryState::RebaseInteractive | 
        git2::RepositoryState::RebaseMerge
    );
    let is_merging = state == git2::RepositoryState::Merge;
    let is_cherry_picking = state == git2::RepositoryState::CherryPick || 
                            state == git2::RepositoryState::CherryPickSequence;
    let is_reverting = state == git2::RepositoryState::Revert || 
                       state == git2::RepositoryState::RevertSequence;
    let is_bisecting = state == git2::RepositoryState::Bisect;
    
    // Get conflicts
    let index = repo.index()?;
    let conflicts: Vec<PathBuf> = if index.has_conflicts() {
        index.conflicts()?
            .filter_map(|c| c.ok())
            .filter_map(|c| c.our.or(c.their).map(|e| PathBuf::from(String::from_utf8_lossy(&e.path).to_string())))
            .collect()
    } else {
        Vec::new()
    };
    
    Ok(GitStatus {
        is_rebasing,
        is_merging,
        is_cherry_picking,
        is_reverting,
        is_bisecting,
        rebase_head_name: None, // TODO: Read from .git/rebase-merge/head-name
        merge_head: None, // TODO: Read from .git/MERGE_HEAD
        conflicts,
    })
}

// ============================================================================
// Git Stage Operations
// ============================================================================

fn git_stage_files(workspace_path: &Path, paths: &[PathBuf]) -> Result<()> {
    let repo = Repository::discover(workspace_path)?;
    let mut index = repo.index()?;
    
    for path in paths {
        let relative = path.strip_prefix(workspace_path).unwrap_or(path);
        index.add_path(relative)?;
    }
    
    index.write()?;
    Ok(())
}

fn git_unstage_files(workspace_path: &Path, paths: &[PathBuf]) -> Result<()> {
    let repo = Repository::discover(workspace_path)?;
    let head = repo.head()?.peel_to_commit()?;
    
    for path in paths {
        let relative = path.strip_prefix(workspace_path).unwrap_or(path);
        repo.reset_default(Some(head.as_object()), &[relative])?;
    }
    
    Ok(())
}

fn git_stage_all(workspace_path: &Path) -> Result<()> {
    let repo = Repository::discover(workspace_path)?;
    let mut index = repo.index()?;
    index.add_all(["*"].iter(), git2::IndexAddOption::DEFAULT, None)?;
    index.write()?;
    Ok(())
}

fn git_unstage_all(workspace_path: &Path) -> Result<()> {
    let repo = Repository::discover(workspace_path)?;
    let head = repo.head()?.peel_to_commit()?;
    repo.reset(head.as_object(), git2::ResetType::Mixed, None)?;
    Ok(())
}

fn file_get_head(workspace_path: &Path, path: &Path) -> Result<(String, String)> {
    let repo = Repository::discover(workspace_path)?;
    let head = repo.head()?;
    let tree = head.peel_to_tree()?;
    let tree_entry = tree.get_path(path.strip_prefix(workspace_path)?)?;
    let blob = repo.find_blob(tree_entry.id())?;
    let id = blob.id().to_string();
    let content = std::str::from_utf8(blob.content())
        .with_context(|| "content bytes to string")?
        .to_string();
    Ok((id, content))
}

fn git_get_remote_file_url(workspace_path: &Path, file: &Path) -> Result<String> {
    let repo = Repository::discover(workspace_path)?;
    let head = repo.head()?;
    let target_remote = repo.find_remote(
        repo.branch_upstream_remote(head.name().unwrap())?
            .as_str()
            .unwrap(),
    )?;

    // Grab URL part of remote
    let remote = target_remote
        .url()
        .ok_or(anyhow!("Failed to convert remote to str"))?;

    let remote_url = match Url::parse(remote) {
        Ok(url) => url,
        Err(_) => {
            // Parse URL as ssh
            Url::parse(&format!("ssh://{}", remote.replacen(':', "/", 1)))?
        }
    };

    // Get host part
    let host = remote_url
        .host_str()
        .ok_or(anyhow!("Couldn't find remote host"))?;
    // Get namespace (e.g. organisation/project in case of GitHub, org/team/team/team/../project on GitLab)
    let namespace = if let Some(stripped) = remote_url.path().strip_suffix(".git") {
        stripped
    } else {
        remote_url.path()
    };

    let commit = head.peel_to_commit()?.id();

    let file_path = file
        .strip_prefix(workspace_path)?
        .to_str()
        .ok_or(anyhow!("Couldn't convert file path to str"))?;

    let url = format!("https://{host}{namespace}/blob/{commit}/{file_path}",);

    Ok(url)
}

fn search_in_path(
    id: u64,
    current_id: &AtomicU64,
    paths: impl Iterator<Item = PathBuf>,
    pattern: &str,
    case_sensitive: bool,
    whole_word: bool,
    is_regex: bool,
) -> Result<ProxyResponse, RpcError> {
    let mut matches = IndexMap::new();
    let mut matcher = RegexMatcherBuilder::new();
    let matcher = matcher.case_insensitive(!case_sensitive).word(whole_word);
    let matcher = if is_regex {
        matcher.build(pattern)
    } else {
        matcher.build_literals(&[&regex::escape(pattern)])
    };
    let matcher = matcher.map_err(|_| RpcError {
        code: 0,
        message: "can't build matcher".to_string(),
    })?;
    let mut searcher = SearcherBuilder::new().build();

    for path in paths {
        if current_id.load(Ordering::SeqCst) != id {
            return Err(RpcError {
                code: 0,
                message: "expired search job".to_string(),
            });
        }

        if path.is_file() {
            let mut line_matches = Vec::new();
            if let Err(err) = searcher.search_path(
                &matcher,
                path.clone(),
                UTF8(|lnum, line| {
                    if current_id.load(Ordering::SeqCst) != id {
                        return Ok(false);
                    }

                    let mymatch = matcher.find(line.as_bytes())?.unwrap();
                    let line = if line.len() > 200 {
                        // Shorten the line to avoid sending over absurdly long-lines
                        // (such as in minified javascript)
                        // Note that the start/end are column based, not absolute from the
                        // start of the file.
                        let left_keep = line[..mymatch.start()]
                            .chars()
                            .rev()
                            .take(100)
                            .map(|c| c.len_utf8())
                            .sum::<usize>();
                        let right_keep = line[mymatch.end()..]
                            .chars()
                            .take(100)
                            .map(|c| c.len_utf8())
                            .sum::<usize>();
                        let display_range =
                            mymatch.start() - left_keep..mymatch.end() + right_keep;
                        line[display_range].to_string()
                    } else {
                        line.to_string()
                    };
                    line_matches.push(SearchMatch {
                        line: lnum as usize,
                        start: mymatch.start(),
                        end: mymatch.end(),
                        line_content: line,
                    });
                    Ok(true)
                }),
            ) {
                {
                    tracing::error!("{:?}", err);
                }
            }
            if !line_matches.is_empty() {
                matches.insert(path.clone(), line_matches);
            }
        }
    }

    Ok(ProxyResponse::GlobalSearchResponse { matches })
}
