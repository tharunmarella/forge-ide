use std::{
    collections::HashMap,
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
// git2 dependency removed - using gix and git command line instead
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
    agent_terminal::AgentTerminalManager,
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
    terminals: Arc<std::sync::Mutex<HashMap<TermId, TerminalSender>>>,
    file_watcher: FileWatcher,
    window_id: usize,
    tab_id: usize,
    db_manager: crate::database::connection_manager::ConnectionManager,
    /// Stores pre-edit file snapshots keyed by diff_id: (relative_path, old_content).
    /// Used to revert files when the user rejects an AI-proposed diff.
    pending_diff_snapshots: Arc<Mutex<HashMap<String, (String, String)>>>,
    /// Pending approval channels: tool_call_id -> oneshot sender (true=approved, false=rejected).
    /// The agent loop awaits on the receiver; the UI sends approve/reject via ProxyRequest.
    pending_approvals: Arc<Mutex<HashMap<String, tokio::sync::oneshot::Sender<bool>>>>,
    /// Manages terminals created by the AI agent (visible in terminal panel).
    agent_terminal_mgr: Arc<AgentTerminalManager>,
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
                for (_, sender) in self.terminals.lock().unwrap().iter() {
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
                self.terminals.lock().unwrap().insert(term_id, sender);
                let rpc = self.core_rpc.clone();
                thread::spawn(move || {
                    terminal.run(rpc);
                });
            }
            TerminalWrite { term_id, content } => {
                if let Some(tx) = self.terminals.lock().unwrap().get(&term_id) {
                    tx.send(Msg::Input(content.into_bytes().into()));
                }
            }
            TerminalResize {
                term_id,
                width,
                height,
            } => {
                if let Some(tx) = self.terminals.lock().unwrap().get(&term_id) {
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
                if let Some(tx) = self.terminals.lock().unwrap().remove(&term_id) {
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
                    let file_count = diffs.len();
                    match git_commit(workspace, &message, diffs) {
                        Ok(()) => {
                            let msg = if file_count == 1 {
                                "1 file committed successfully.".to_string()
                            } else {
                                format!("{} files committed successfully.", file_count)
                            };
                            self.core_rpc.show_message(
                                "Commit Successful".to_owned(),
                                ShowMessageParams {
                                    typ: MessageType::INFO,
                                    message: msg,
                                },
                            );
                        }
                        Err(e) => {
                            self.core_rpc.show_message(
                                "Commit Failed".to_owned(),
                                ShowMessageParams {
                                    typ: MessageType::ERROR,
                                    message: e.to_string(),
                                },
                            );
                        }
                    }
                }
            }
            GitCommitAndPush { message, diffs } => {
                if let Some(workspace) = self.workspace.as_ref() {
                    let file_count = diffs.len();
                    match git_commit(workspace, &message, diffs) {
                        Ok(()) => {
                            // Now push
                            let push_result = std::process::Command::new("git")
                                .arg("push")
                                .current_dir(workspace)
                                .output();
                            
                            match push_result {
                                Ok(output) => {
                                    let stdout = String::from_utf8_lossy(&output.stdout);
                                    let stderr = String::from_utf8_lossy(&output.stderr);
                                    let combined = format!("{}{}", stdout, stderr);
                                    
                                    if output.status.success() {
                                        let commit_msg = if file_count == 1 {
                                            "1 file committed".to_string()
                                        } else {
                                            format!("{} files committed", file_count)
                                        };
                                        
                                        let push_msg = if combined.contains("Everything up-to-date") {
                                            "already synced with remote".to_string()
                                        } else {
                                            "pushed to remote".to_string()
                                        };
                                        
                                        self.core_rpc.show_message(
                                            "Commit & Push Successful".to_owned(),
                                            ShowMessageParams {
                                                typ: MessageType::INFO,
                                                message: format!("{} and {}.", commit_msg, push_msg),
                                            },
                                        );
                                    } else {
                                        self.core_rpc.show_message(
                                            "Push Failed".to_owned(),
                                            ShowMessageParams {
                                                typ: MessageType::ERROR,
                                                message: format!("Commit succeeded but push failed: {}", stderr),
                                            },
                                        );
                                    }
                                }
                                Err(e) => {
                                    self.core_rpc.show_message(
                                        "Push Failed".to_owned(),
                                        ShowMessageParams {
                                            typ: MessageType::ERROR,
                                            message: format!("Commit succeeded but push failed: {}", e),
                                        },
                                    );
                                }
                            }
                        }
                        Err(e) => {
                            self.core_rpc.show_message(
                                "Commit Failed".to_owned(),
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
                tracing::info!("[GIT CHECKOUT PROXY] Received GitCheckout notification for reference: {}", reference);
                if let Some(workspace) = self.workspace.as_ref() {
                    tracing::info!("[GIT CHECKOUT PROXY] Workspace path: {:?}", workspace);
                    match git_checkout(workspace, &reference) {
                        Ok(()) => {
                            tracing::info!("[GIT CHECKOUT PROXY] SUCCESS: Checkout completed for {}", reference);
                        }
                        Err(e) => {
                            tracing::error!("[GIT CHECKOUT PROXY] ERROR: Checkout failed: {:?}", e);
                        }
                    }
                } else {
                    tracing::error!("[GIT CHECKOUT PROXY] ERROR: No workspace available");
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
                let path = self.resolve_path(path);
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
                eprintln!("[GIT_LOG] Request received: limit={}, skip={}", limit, skip);
                let result = if let Some(workspace) = self.workspace.as_ref() {
                    eprintln!("[GIT_LOG] Workspace: {:?}", workspace);
                    match git_log(workspace, limit, skip, branch, author, search) {
                        Ok(result) => {
                            eprintln!("[GIT_LOG] Success: {} commits found, total: {}", result.commits.len(), result.total_count);
                            Ok(ProxyResponse::GitLogResponse { result })
                        },
                        Err(e) => {
                            eprintln!("[GIT_LOG] Error: {}", e);
                            Err(RpcError {
                                code: 0,
                                message: format!("git log error: {}", e),
                            })
                        },
                    }
                } else {
                    eprintln!("[GIT_LOG] No workspace set!");
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
            // Git Checkout Operations
            GitCheckoutRequest { reference } => {
                let result = if let Some(workspace) = self.workspace.as_ref() {
                    match git_checkout_with_status(workspace, &reference) {
                        Ok(result) => Ok(ProxyResponse::GitCheckoutResponse { result }),
                        Err(e) => Err(RpcError { code: 0, message: format!("git checkout error: {}", e) }),
                    }
                } else {
                    Err(RpcError { code: 0, message: "no workspace set".to_string() })
                };
                self.respond_rpc(id, result);
            }
            GitSmartCheckout { reference } => {
                let result = if let Some(workspace) = self.workspace.as_ref() {
                    match git_smart_checkout(workspace, &reference) {
                        Ok(result) => Ok(ProxyResponse::GitCheckoutResponse { result }),
                        Err(e) => Err(RpcError { code: 0, message: format!("git smart checkout error: {}", e) }),
                    }
                } else {
                    Err(RpcError { code: 0, message: "no workspace set".to_string() })
                };
                self.respond_rpc(id, result);
            }
            GitForceCheckout { reference } => {
                let result = if let Some(workspace) = self.workspace.as_ref() {
                    match git_force_checkout(workspace, &reference) {
                        Ok(result) => Ok(ProxyResponse::GitCheckoutResponse { result }),
                        Err(e) => Err(RpcError { code: 0, message: format!("git force checkout error: {}", e) }),
                    }
                } else {
                    Err(RpcError { code: 0, message: "no workspace set".to_string() })
                };
                self.respond_rpc(id, result);
            }
            // Git Push/Pull/Fetch - These require network operations, placeholder for now
            GitPush { options: _ } => {
                // TODO: Implement push using command-line git
                self.respond_rpc(id, Err(RpcError { 
                    code: 0, 
                    message: "Push requires SSH/HTTPS credentials - use terminal for now".to_string() 
                }));
            }
            GitPull { options: _ } => {
                // TODO: Implement pull using command-line git
                self.respond_rpc(id, Err(RpcError { 
                    code: 0, 
                    message: "Pull requires SSH/HTTPS credentials - use terminal for now".to_string() 
                }));
            }
            GitFetch { options: _ } => {
                // TODO: Implement fetch using command-line git
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
                    .filter_map(|(path, buffer)| {
                        let abs_path = self.resolve_path(path.clone());
                        let uri = Url::from_file_path(&abs_path).ok()?;
                        Some(TextDocumentItem {
                            uri,
                            language_id: buffer.language_id.to_string(),
                            version: buffer.rev as i32,
                            text: buffer.get_document(),
                        })
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
                let rope_clone = buffer.rope.clone();
                let path_clone = path.clone();
                let workspace = self.workspace.clone();
                
                let result = buffer
                    .save(rev, create_parents)
                    .map(|_r| {
                        self.catalog_rpc
                            .did_save_text_document(&path, rope_clone.clone());
                        
                        // ── Incremental Re-index on Save ──
                        // Fire-and-forget: update the cloud index for this file
                        if forge_agent::forge_search::is_indexable_file(
                            &path_clone.file_name()
                                .map(|n| n.to_string_lossy().to_string())
                                .unwrap_or_default()
                        ) {
                            let file_content = rope_clone.to_string();
                            let file_path = path_clone.clone();
                            let ws = workspace.clone();
                            
                            tokio::spawn(async move {
                                let workspace_id = ws
                                    .as_ref()
                                    .and_then(|p| p.file_name())
                                    .and_then(|n| n.to_str())
                                    .unwrap_or("default");
                                
                                let rel_path = ws.as_ref()
                                    .and_then(|ws| file_path.strip_prefix(ws).ok())
                                    .map(|p| p.display().to_string())
                                    .unwrap_or_else(|| file_path.display().to_string());
                                
                                let client = forge_agent::forge_search::client();
                                if let Err(e) = client.index_file(
                                    workspace_id,
                                    &rel_path,
                                    &file_content
                                ).await {
                                    tracing::debug!("Incremental index failed for {}: {}", rel_path, e);
                                } else {
                                    tracing::trace!("Re-indexed {}", rel_path);
                                }
                            });
                        }
                        
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
                let path = self.resolve_path(path);
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
            // Proto SDK Manager
            ProtoIsInstalled {} => {
                use crate::proto_manager::ProtoManager;
                let installed = ProtoManager::is_proto_installed();
                self.respond_rpc(id, Ok(ProxyResponse::ProtoIsInstalledResponse { installed }));
            }
            ProtoGetVersion {} => {
                use crate::proto_manager::ProtoManager;
                let result = match ProtoManager::get_proto_version() {
                    Ok(version) => Ok(ProxyResponse::ProtoVersionResponse { version }),
                    Err(e) => Err(RpcError { code: 0, message: format!("Failed to get proto version: {}", e) }),
                };
                self.respond_rpc(id, result);
            }
            ProtoListTools {} => {
                use crate::proto_manager::ProtoManager;
                let manager = ProtoManager::new(self.workspace.clone());
                let result = match manager.list_installed_tools() {
                    Ok(tools) => {
                        let tools = tools.into_iter().map(|t| lapce_rpc::proxy::ProtoToolInfo {
                            name: t.name,
                            version: t.version,
                            path: t.path,
                            is_default: t.is_default,
                        }).collect();
                        Ok(ProxyResponse::ProtoToolsListResponse { tools })
                    }
                    Err(e) => Err(RpcError { code: 0, message: format!("Failed to list tools: {}", e) }),
                };
                self.respond_rpc(id, result);
            }
            ProtoInstallTool { tool, version } => {
                use crate::proto_manager::ProtoManager;
                let manager = ProtoManager::new(self.workspace.clone());
                let result = match manager.install_tool(&tool, &version) {
                    Ok(message) => Ok(ProxyResponse::ProtoInstallResponse { success: true, message }),
                    Err(e) => Ok(ProxyResponse::ProtoInstallResponse { 
                        success: false, 
                        message: format!("Failed to install {} {}: {}", tool, version, e) 
                    }),
                };
                self.respond_rpc(id, result);
            }
            ProtoUninstallTool { tool, version } => {
                use crate::proto_manager::ProtoManager;
                let manager = ProtoManager::new(self.workspace.clone());
                let result = match manager.uninstall_tool(&tool, &version) {
                    Ok(message) => Ok(ProxyResponse::ProtoInstallResponse { success: true, message }),
                    Err(e) => Ok(ProxyResponse::ProtoInstallResponse { 
                        success: false, 
                        message: format!("Failed to uninstall {} {}: {}", tool, version, e) 
                    }),
                };
                self.respond_rpc(id, result);
            }
            ProtoGetToolPath { tool } => {
                use crate::proto_manager::ProtoManager;
                let manager = ProtoManager::new(self.workspace.clone());
                let path = manager.get_tool_bin_path(&tool).ok();
                self.respond_rpc(id, Ok(ProxyResponse::ProtoToolPathResponse { path }));
            }
            ProtoListRemoteVersions { tool } => {
                use crate::proto_manager::ProtoManager;
                let manager = ProtoManager::new(self.workspace.clone());
                let result = match manager.search_tool_versions(&tool) {
                    Ok(versions) => Ok(ProxyResponse::ProtoVersionsListResponse { versions }),
                    Err(e) => Err(RpcError { code: 0, message: format!("Failed to list versions for {}: {}", tool, e) }),
                };
                self.respond_rpc(id, result);
            }
            ProtoGetProjectConfig {} => {
                use crate::proto_manager::ProtoManager;
                let manager = ProtoManager::new(self.workspace.clone());
                let result = match manager.read_project_config() {
                    Ok(config) => Ok(ProxyResponse::ProtoProjectConfigResponse { tools: config.tools }),
                    Err(e) => Err(RpcError { code: 0, message: format!("Failed to read project config: {}", e) }),
                };
                self.respond_rpc(id, result);
            }
            ProtoSetupProject {} => {
                use crate::proto_manager::ProtoManager;
                let manager = ProtoManager::new(self.workspace.clone());
                let result = match manager.setup_project() {
                    Ok(message) => Ok(ProxyResponse::ProtoInstallResponse { success: true, message }),
                    Err(e) => Ok(ProxyResponse::ProtoInstallResponse { 
                        success: false, 
                        message: format!("Failed to setup project: {}", e) 
                    }),
                };
                self.respond_rpc(id, result);
            }
            ProtoDetectProjectTools {} => {
                use crate::proto_manager::ProtoManager;
                let tools = if let Some(workspace) = self.workspace.as_ref() {
                    ProtoManager::detect_project_tools(workspace)
                } else {
                    Vec::new()
                };
                self.respond_rpc(id, Ok(ProxyResponse::ProtoDetectedToolsResponse { tools }));
            }
            
            // Run Configuration Detection
            DetectRunConfigs {} => {
                use crate::run_config_detector::detect_run_configs;
                let configs = if let Some(workspace) = self.workspace.as_ref() {
                    detect_run_configs(workspace)
                } else {
                    Vec::new()
                };
                self.respond_rpc(id, Ok(ProxyResponse::DetectedRunConfigsResponse { configs }));
            }
            
            GetRunConfigs {} => {
                use crate::run_config_detector::detect_run_configs;
                
                tracing::info!("GetRunConfigs: Starting");
                
                let detected = if let Some(workspace) = self.workspace.as_ref() {
                    tracing::info!("GetRunConfigs: Detecting configs in {:?}", workspace);
                    detect_run_configs(workspace)
                } else {
                    tracing::info!("GetRunConfigs: No workspace");
                    Vec::new()
                };
                
                tracing::info!("GetRunConfigs: Found {} detected configs", detected.len());
                
                // Load user configs from .lapce/run.toml
                let user = if let Some(workspace) = self.workspace.as_ref() {
                    let run_toml = workspace.join(".lapce").join("run.toml");
                    if run_toml.exists() {
                        tracing::info!("GetRunConfigs: Loading user configs from {:?}", run_toml);
                        if let Ok(content) = std::fs::read_to_string(&run_toml) {
                            #[derive(serde::Deserialize)]
                            struct RunConfigs {
                                configs: Vec<lapce_rpc::dap_types::RunDebugConfig>,
                            }
                            toml::from_str::<RunConfigs>(&content)
                                .map(|c| c.configs)
                                .unwrap_or_default()
                        } else {
                            Vec::new()
                        }
                    } else {
                        tracing::info!("GetRunConfigs: No run.toml file");
                        Vec::new()
                    }
                } else {
                    Vec::new()
                };
                
                tracing::info!("GetRunConfigs: Responding with {} detected, {} user configs", detected.len(), user.len());
                self.respond_rpc(id, Ok(ProxyResponse::RunConfigsResponse { detected, user }));
            }
            
            SaveRunConfig { config } => {
                let result = if let Some(workspace) = self.workspace.as_ref() {
                    let lapce_dir = workspace.join(".lapce");
                    if !lapce_dir.exists() {
                        let _ = std::fs::create_dir_all(&lapce_dir);
                    }
                    let run_toml = lapce_dir.join("run.toml");
                    
                    // Load existing configs
                    #[derive(serde::Deserialize, serde::Serialize)]
                    struct RunConfigs {
                        configs: Vec<lapce_rpc::dap_types::RunDebugConfig>,
                    }
                    
                    let mut configs = if run_toml.exists() {
                        std::fs::read_to_string(&run_toml)
                            .ok()
                            .and_then(|c| toml::from_str::<RunConfigs>(&c).ok())
                            .map(|c| c.configs)
                            .unwrap_or_default()
                    } else {
                        Vec::new()
                    };
                    
                    // Update or add config
                    if let Some(pos) = configs.iter().position(|c| c.name == config.name) {
                        configs[pos] = config;
                    } else {
                        configs.push(config);
                    }
                    
                    // Save
                    match toml::to_string_pretty(&RunConfigs { configs }) {
                        Ok(content) => {
                            match std::fs::write(&run_toml, content) {
                                Ok(_) => (true, "Configuration saved".to_string()),
                                Err(e) => (false, format!("Failed to save: {}", e)),
                            }
                        }
                        Err(e) => (false, format!("Failed to serialize: {}", e)),
                    }
                } else {
                    (false, "No workspace open".to_string())
                };
                
                self.respond_rpc(id, Ok(ProxyResponse::RunConfigSaveResponse { 
                    success: result.0, 
                    message: result.1 
                }));
            }
            
            DeleteRunConfig { name } => {
                let result = if let Some(workspace) = self.workspace.as_ref() {
                    let run_toml = workspace.join(".lapce").join("run.toml");
                    
                    #[derive(serde::Deserialize, serde::Serialize)]
                    struct RunConfigs {
                        configs: Vec<lapce_rpc::dap_types::RunDebugConfig>,
                    }
                    
                    if run_toml.exists() {
                        let mut configs = std::fs::read_to_string(&run_toml)
                            .ok()
                            .and_then(|c| toml::from_str::<RunConfigs>(&c).ok())
                            .map(|c| c.configs)
                            .unwrap_or_default();
                        
                        configs.retain(|c| c.name != name);
                        
                        match toml::to_string_pretty(&RunConfigs { configs }) {
                            Ok(content) => {
                                match std::fs::write(&run_toml, content) {
                                    Ok(_) => (true, "Configuration deleted".to_string()),
                                    Err(e) => (false, format!("Failed to save: {}", e)),
                                }
                            }
                            Err(e) => (false, format!("Failed to serialize: {}", e)),
                        }
                    } else {
                        (true, "No configurations to delete".to_string())
                    }
                } else {
                    (false, "No workspace open".to_string())
                };
                
                self.respond_rpc(id, Ok(ProxyResponse::RunConfigSaveResponse { 
                    success: result.0, 
                    message: result.1 
                }));
            }

            // Database Manager handlers
            DbListConnections {} => {
                let connections = crate::database::connection_manager::ConnectionManager::load_connections()
                    .unwrap_or_default();
                self.respond_rpc(
                    id,
                    Ok(ProxyResponse::DbConnectionsListResponse { connections }),
                );
            }
            DbSaveConnection { config } => {
                match crate::database::connection_manager::ConnectionManager::save_connection(config) {
                    Ok(()) => {
                        let connections = crate::database::connection_manager::ConnectionManager::load_connections()
                            .unwrap_or_default();
                        self.respond_rpc(
                            id,
                            Ok(ProxyResponse::DbConnectionsListResponse { connections }),
                        );
                    }
                    Err(e) => {
                        self.respond_rpc(
                            id,
                            Err(RpcError {
                                code: 0,
                                message: format!("Failed to save connection: {}", e),
                            }),
                        );
                    }
                }
            }
            DbDeleteConnection { id: conn_id } => {
                self.db_manager.disconnect(&conn_id);
                match crate::database::connection_manager::ConnectionManager::delete_connection(&conn_id) {
                    Ok(()) => {
                        let connections = crate::database::connection_manager::ConnectionManager::load_connections()
                            .unwrap_or_default();
                        self.respond_rpc(
                            id,
                            Ok(ProxyResponse::DbConnectionsListResponse { connections }),
                        );
                    }
                    Err(e) => {
                        self.respond_rpc(
                            id,
                            Err(RpcError {
                                code: 0,
                                message: format!("Failed to delete connection: {}", e),
                            }),
                        );
                    }
                }
            }
            DbTestConnection { config } => {
                match self.db_manager.test_connection(&config) {
                    Ok(true) => {
                        self.respond_rpc(
                            id,
                            Ok(ProxyResponse::DbTestConnectionResponse {
                                success: true,
                                message: "Connection successful".to_string(),
                            }),
                        );
                    }
                    Ok(false) | Err(_) => {
                        let message = "Connection failed".to_string();
                        self.respond_rpc(
                            id,
                            Ok(ProxyResponse::DbTestConnectionResponse {
                                success: false,
                                message,
                            }),
                        );
                    }
                }
            }
            DbConnect { connection_id } => {
                match self.db_manager.connect(&connection_id) {
                    Ok(schema) => {
                        self.respond_rpc(
                            id,
                            Ok(ProxyResponse::DbConnectResponse {
                                success: true,
                                schema: Some(schema),
                                message: "Connected".to_string(),
                            }),
                        );
                    }
                    Err(e) => {
                        self.respond_rpc(
                            id,
                            Ok(ProxyResponse::DbConnectResponse {
                                success: false,
                                schema: None,
                                message: format!("Connection failed: {}", e),
                            }),
                        );
                    }
                }
            }
            DbDisconnect { connection_id } => {
                self.db_manager.disconnect(&connection_id);
                self.respond_rpc(
                    id,
                    Ok(ProxyResponse::DbTestConnectionResponse {
                        success: true,
                        message: "Disconnected".to_string(),
                    }),
                );
            }
            DbGetSchema { connection_id } => {
                match self.db_manager.get_schema(&connection_id) {
                    Ok(schema) => {
                        self.respond_rpc(
                            id,
                            Ok(ProxyResponse::DbSchemaResponse { schema }),
                        );
                    }
                    Err(e) => {
                        self.respond_rpc(
                            id,
                            Err(RpcError {
                                code: 0,
                                message: format!("Failed to get schema: {}", e),
                            }),
                        );
                    }
                }
            }
            DbGetTableData {
                connection_id,
                table,
                offset,
                limit,
            } => {
                match self.db_manager.get_table_data(&connection_id, &table, offset, limit) {
                    Ok(result) => {
                        self.respond_rpc(
                            id,
                            Ok(ProxyResponse::DbQueryResponse { result }),
                        );
                    }
                    Err(e) => {
                        self.respond_rpc(
                            id,
                            Err(RpcError {
                                code: 0,
                                message: format!("Failed to get table data: {}", e),
                            }),
                        );
                    }
                }
            }
            DbGetTableStructure {
                connection_id,
                table,
            } => {
                match self.db_manager.get_table_structure(&connection_id, &table) {
                    Ok(structure) => {
                        self.respond_rpc(
                            id,
                            Ok(ProxyResponse::DbTableStructureResponse { structure }),
                        );
                    }
                    Err(e) => {
                        self.respond_rpc(
                            id,
                            Err(RpcError {
                                code: 0,
                                message: format!("Failed to get table structure: {}", e),
                            }),
                        );
                    }
                }
            }
            DbExecuteQuery {
                connection_id,
                query,
            } => {
                match self.db_manager.execute_query(&connection_id, &query) {
                    Ok(result) => {
                        self.respond_rpc(
                            id,
                            Ok(ProxyResponse::DbQueryResponse { result }),
                        );
                    }
                    Err(e) => {
                        self.respond_rpc(
                            id,
                            Err(RpcError {
                                code: 0,
                                message: format!("Query execution failed: {}", e),
                            }),
                        );
                    }
                }
            }

            // ── AI Agent ─────────────────────────────────────────
            AgentPrompt { prompt, provider, model, api_key, conversation_id: conv_id, attached_images } => {
                tracing::info!("Agent prompt received, conv_id={conv_id}, provider={provider}, model={model}");
                let proxy_rpc = self.proxy_rpc.clone();
                let core_rpc = self.core_rpc.clone();
                let workspace = self.workspace.clone();
                let _diff_snapshots = self.pending_diff_snapshots.clone();
                let pending_approvals = self.pending_approvals.clone();
                let agent_term_mgr = self.agent_terminal_mgr.clone();
                let ide_terminals = self.terminals.clone();
                let _ = (provider, model, api_key); // Unused — all LLM calls go through forge-search

                thread::spawn(move || {
                    let rt = match tokio::runtime::Runtime::new() {
                        Ok(rt) => rt,
                        Err(e) => {
                            proxy_rpc.handle_response(
                                id,
                                Ok(ProxyResponse::AgentError {
                                    error: format!("Failed to create async runtime: {e}"),
                                }),
                            );
                            return;
                        }
                    };

                    rt.block_on(async move {
                        let workspace_path = workspace
                            .unwrap_or_else(|| std::env::current_dir().unwrap_or_default());

                        // ══════════════════════════════════════════════════════
                        // All LLM calls go through forge-search cloud.
                        // Uses the /chat endpoint (JSON, not SSE) with multi-turn tool loop.
                        // ══════════════════════════════════════════════════════
                        let workspace_name = workspace_path
                            .file_name()
                            .map(|n| n.to_string_lossy().to_string())
                            .unwrap_or_else(|| "default".to_string());

                        let fs_client = forge_agent::forge_search::client();
                        
                        // Auto-Index
                        core_rpc.notification(CoreNotification::AgentToolCallUpdate {
                            tool_call_id: "_indexing".to_string(),
                            tool_name: "Checking workspace index...".to_string(),
                            arguments: String::new(),
                            status: "pending".to_string(),
                            output: None,
                        });
                        
                        let (was_indexed, symbol_count) = 
                            forge_agent::tools::ensure_indexed(&workspace_path).await;
                        
                        let index_msg = if was_indexed {
                            format!("Workspace ready ({} symbols indexed)", symbol_count)
                        } else if symbol_count > 0 {
                            format!("Indexed {} symbols", symbol_count)
                        } else {
                            "Workspace indexed".to_string()
                        };
                        
                        core_rpc.notification(CoreNotification::AgentToolCallUpdate {
                            tool_call_id: "_indexing".to_string(),
                            tool_name: index_msg,
                            arguments: String::new(),
                            status: "success".to_string(),
                            output: None,
                        });
                        
                        let attached_files = collect_relevant_files(&workspace_path);
                        let conversation_id = format!("{}-{}", workspace_name, conv_id);
                        let mut tool_results: Vec<serde_json::Value> = Vec::new();
                        let mut is_first_turn = true;
                        let mut turn = 0;
                        
                        loop {
                            turn += 1;
                            let mut chat_req = serde_json::json!({
                                "workspace_id": workspace_name,
                                "conversation_id": conversation_id,
                            });
                            
                            if is_first_turn {
                                chat_req["question"] = serde_json::Value::String(prompt.clone());
                                if !attached_files.is_empty() {
                                    chat_req["attached_files"] = serde_json::json!(attached_files);
                                }
                                // Include pasted/attached images (base64)
                                if !attached_images.is_empty() {
                                    let images_json: Vec<serde_json::Value> = attached_images.iter().map(|img| {
                                        serde_json::json!({
                                            "filename": img.filename,
                                            "data": img.data,
                                            "mime_type": img.mime_type,
                                        })
                                    }).collect();
                                    chat_req["attached_images"] = serde_json::Value::Array(images_json);
                                }
                                is_first_turn = false;
                            }
                            
                            if !tool_results.is_empty() {
                                chat_req["tool_results"] = serde_json::Value::Array(tool_results.clone());
                                tool_results.clear();
                            }

                            tracing::info!("Cloud chat turn {} for {}", turn, conversation_id);

                            // ── Non-streaming JSON request to /chat ──
                            match fs_client.chat_with_body(&chat_req).await {
                                Ok(response) => {
                                    // Parse the response - it's a JSON object with answer, tool_calls, status
                                    let answer = response.get("answer").and_then(|a| a.as_str()).unwrap_or("");
                                    let status = response.get("status").and_then(|s| s.as_str()).unwrap_or("done");
                                    let server_tool_calls = response.get("tool_calls").and_then(|t| t.as_array());
                                    
                                    // Forward plan steps to IDE (sent on every turn so the UI updates as steps complete)
                                    if let Some(plan_arr) = response.get("plan_steps").and_then(|p| p.as_array()) {
                                        let steps: Vec<lapce_rpc::core::AgentPlanStep> = plan_arr
                                            .iter()
                                            .filter_map(|s| {
                                                let number = s.get("number").and_then(|n| n.as_u64())? as u32;
                                                let description = s.get("description").and_then(|d| d.as_str())?.to_string();
                                                let status_str = s.get("status").and_then(|st| st.as_str()).unwrap_or("pending");
                                                let step_status = match status_str {
                                                    "in_progress" => lapce_rpc::core::AgentPlanStepStatus::InProgress,
                                                    "done" => lapce_rpc::core::AgentPlanStepStatus::Done,
                                                    _ => lapce_rpc::core::AgentPlanStepStatus::Pending,
                                                };
                                                Some(lapce_rpc::core::AgentPlanStep {
                                                    number,
                                                    description,
                                                    status: step_status,
                                                })
                                            })
                                            .collect();
                                        if !steps.is_empty() {
                                            tracing::info!("Forwarding {} plan steps to IDE", steps.len());
                                            core_rpc.agent_plan(steps);
                                        }
                                    }
                                    
                                    // Send answer text to UI
                                    if !answer.is_empty() {
                                        core_rpc.agent_text_chunk(answer.to_string(), false);
                                    }
                                    
                                    // Check for tool calls that need IDE execution
                                    if status == "requires_action" {
                                        if let Some(tcs) = server_tool_calls {
                                            let mut has_tool_calls = false;
                                            for tc_val in tcs {
                                                let tc_id = tc_val.get("id").and_then(|i| i.as_str()).unwrap_or("").to_string();
                                                let tc_name = tc_val.get("name").and_then(|n| n.as_str()).unwrap_or("").to_string();
                                                let tc_args = tc_val.get("args").cloned().unwrap_or(serde_json::Value::Object(Default::default()));
                                                
                                                if tc_name.is_empty() { continue; }
                                                has_tool_calls = true;
                                                
                                                let args_json = serde_json::to_string(&tc_args).unwrap_or_default();
                                                
                                                // ── Smart approval: only ask for genuinely risky operations ──
                                                // Tier 1 (auto-approve): read-only tools, safe commands
                                                // Tier 2 (needs approval): file writes, deletes, risky commands
                                                
                                                let cmd_str = tc_args.get("command").and_then(|c| c.as_str()).unwrap_or("");
                                                
                                                // Safe commands that don't need approval
                                                let is_safe_command = if tc_name == "execute_command" || tc_name == "execute_background" {
                                                    let cmd_lower = cmd_str.to_lowercase();
                                                    // Build/test/check commands are safe
                                                    cmd_lower.starts_with("npm run ")
                                                        || cmd_lower.starts_with("npm test")
                                                        || cmd_lower.starts_with("npm install")
                                                        || cmd_lower.starts_with("npx tsc")
                                                        || cmd_lower.starts_with("npx ")
                                                        || cmd_lower.starts_with("yarn ")
                                                        || cmd_lower.starts_with("pnpm ")
                                                        || cmd_lower.starts_with("cargo check")
                                                        || cmd_lower.starts_with("cargo build")
                                                        || cmd_lower.starts_with("cargo test")
                                                        || cmd_lower.starts_with("cargo run")
                                                        || cmd_lower.starts_with("go build")
                                                        || cmd_lower.starts_with("go test")
                                                        || cmd_lower.starts_with("go run")
                                                        || cmd_lower.starts_with("python -m py_compile")
                                                        || cmd_lower.starts_with("python -m pytest")
                                                        || cmd_lower.starts_with("python -m mypy")
                                                        || cmd_lower.starts_with("pytest")
                                                        || cmd_lower.starts_with("pip install")
                                                        || cmd_lower.starts_with("git status")
                                                        || cmd_lower.starts_with("git log")
                                                        || cmd_lower.starts_with("git diff")
                                                        || cmd_lower.starts_with("git branch")
                                                        || cmd_lower.starts_with("git show")
                                                        || cmd_lower.starts_with("cat ")
                                                        || cmd_lower.starts_with("ls")
                                                        || cmd_lower.starts_with("pwd")
                                                        || cmd_lower.starts_with("echo ")
                                                        || cmd_lower.starts_with("which ")
                                                        || cmd_lower.starts_with("node -")
                                                        || cmd_lower.starts_with("rustc --")
                                                } else {
                                                    false
                                                };
                                                
                                                let needs_approval = match tc_name.as_str() {
                                                    // Always needs approval: destructive file ops
                                                    "delete_file" => true,
                                                    // File edits: show diff preview (handled elsewhere), need approval
                                                    "write_to_file" | "replace_in_file" | "apply_patch" => true,
                                                    // Commands: only risky ones need approval
                                                    "execute_command" | "execute_background" => !is_safe_command,
                                                    // LSP rename: cross-file mutation
                                                    "lsp_rename" => true,
                                                    // Everything else (read_file, grep, list_files, etc.): auto-approve
                                                    _ => false,
                                                };
                                                
                                                if needs_approval {
                                                    // Build a human-readable summary
                                                    let summary = match tc_name.as_str() {
                                                        "write_to_file" => {
                                                            let path = tc_args.get("path").and_then(|p| p.as_str()).unwrap_or("?");
                                                            format!("Create/write file: {}", path)
                                                        }
                                                        "replace_in_file" => {
                                                            let path = tc_args.get("path").and_then(|p| p.as_str()).unwrap_or("?");
                                                            format!("Edit file: {}", path)
                                                        }
                                                        "apply_patch" => "Apply multi-file patch".to_string(),
                                                        "delete_file" => {
                                                            let path = tc_args.get("path").and_then(|p| p.as_str()).unwrap_or("?");
                                                            format!("Delete: {}", path)
                                                        }
                                                        "execute_command" | "execute_background" => {
                                                            format!("Run: {}", &cmd_str[..cmd_str.len().min(100)])
                                                        }
                                                        "lsp_rename" => {
                                                            let new_name = tc_args.get("new_name").and_then(|n| n.as_str()).unwrap_or("?");
                                                            let path = tc_args.get("path").and_then(|p| p.as_str()).unwrap_or("?");
                                                            format!("Rename symbol in {} → {}", path, new_name)
                                                        }
                                                        _ => format!("Execute: {}", tc_name),
                                                    };
                                                    
                                                    // Send approval request to UI
                                                    core_rpc.notification(CoreNotification::AgentToolCallApprovalRequest {
                                                        tool_call_id: tc_id.clone(),
                                                        tool_name: tc_name.clone(),
                                                        summary: summary.clone(),
                                                        arguments: args_json.clone(),
                                                    });
                                                    
                                                    // Also show as pending tool call in chat
                                                    core_rpc.notification(CoreNotification::AgentToolCallUpdate {
                                                        tool_call_id: tc_id.clone(),
                                                        tool_name: tc_name.clone(),
                                                        arguments: args_json.clone(),
                                                        status: "waiting_approval".to_string(),
                                                        output: Some(summary),
                                                    });
                                                    
                                                    // Wait for user approval/rejection
                                                    let (tx, rx) = tokio::sync::oneshot::channel::<bool>();
                                                    pending_approvals.lock().insert(tc_id.clone(), tx);
                                                    
                                                    let approved = rx.await.unwrap_or(false);
                                                    
                                                    if !approved {
                                                        // User rejected — tell the server
                                                        core_rpc.notification(CoreNotification::AgentToolCallUpdate {
                                                            tool_call_id: tc_id.clone(),
                                                            tool_name: tc_name.clone(),
                                                            arguments: String::new(),
                                                            status: "rejected".to_string(),
                                                            output: Some("Rejected by user".to_string()),
                                                        });
                                                        
                                                        tool_results.push(serde_json::json!({
                                                            "call_id": tc_id,
                                                            "output": "Tool call was rejected by the user. Try a different approach or ask the user what they'd prefer.",
                                                            "success": false,
                                                        }));
                                                        continue;
                                                    }
                                                }
                                                
                                                // Approved (or read-only tool) — execute
                                                core_rpc.notification(CoreNotification::AgentToolCallUpdate {
                                                    tool_call_id: tc_id.clone(),
                                                    tool_name: tc_name.clone(),
                                                    arguments: args_json,
                                                    status: "running".to_string(),
                                                    output: None,
                                                });
                                                
                                                // Execute tool locally
                                                let tc_info = forge_agent::ToolCallInfo {
                                                    id: tc_id.clone(),
                                                    name: tc_name.clone(),
                                                    args: tc_args,
                                                };
                                                let result = execute_ide_tool(
                                                    &tc_info,
                                                    &workspace_path,
                                                    &core_rpc,
                                                    &agent_term_mgr,
                                                    &ide_terminals,
                                                ).await;
                                                
                                                core_rpc.notification(CoreNotification::AgentToolCallUpdate {
                                                    tool_call_id: tc_id.clone(),
                                                    tool_name: tc_name.clone(),
                                                    arguments: String::new(),
                                                    status: if result.success { "completed" } else { "failed" }.to_string(),
                                                    output: Some(result.output.clone()),
                                                });
                                                
                                                tool_results.push(serde_json::json!({
                                                    "call_id": tc_id,
                                                    "output": result.output,
                                                    "success": result.success,
                                                }));
                                            }
                                            
                                            if has_tool_calls {
                                                continue; // Loop back to send results to server
                                            }
                                        }
                                    }
                                    
                                    // Done
                                    core_rpc.agent_text_chunk(String::new(), true);
                                    proxy_rpc.handle_response(id, Ok(ProxyResponse::AgentDone {
                                        message: answer.to_string(),
                                    }));
                                    return;
                                }
                                Err(e) => {
                                    let error = format!("Cloud chat failed: {}", e);
                                    tracing::error!("{}", error);
                                    core_rpc.agent_error(error.clone());
                                    proxy_rpc.handle_response(id, Ok(ProxyResponse::AgentError { error }));
                                    return;
                                }
                            }
                        }
                    });
                });
            }
            AgentCancel {} => {
                // TODO: Cancel running agent task
                tracing::info!("Agent cancel requested");
                self.respond_rpc(id, Ok(ProxyResponse::AgentDone {
                    message: "Cancelled".to_string(),
                }));
            }
            AgentTranscribeAudio { audio_data } => {
                tracing::info!("Audio transcription requested ({} bytes)", audio_data.len());
                let proxy_rpc = self.proxy_rpc.clone();
                
                // Run in a thread — uses blocking reqwest (no async runtime needed)
                thread::spawn(move || {
                    const GROQ_WHISPER_URL: &str = "https://api.groq.com/openai/v1/audio/transcriptions";
                        let groq_key = std::env::var("GROQ_API_KEY").unwrap_or_default();
                        if groq_key.is_empty() {
                            proxy_rpc.handle_response(id, Ok(ProxyResponse::AgentError {
                                error: "GROQ_API_KEY environment variable not set. Required for audio transcription.".to_string(),
                            }));
                            return;
                        }
                    
                    let client = reqwest::blocking::Client::new();
                    
                    let audio_part = reqwest::blocking::multipart::Part::bytes(audio_data)
                        .file_name("audio.wav")
                        .mime_str("audio/wav")
                        .unwrap();
                    
                    let form = reqwest::blocking::multipart::Form::new()
                        .text("model", "whisper-large-v3-turbo")
                        .text("temperature", "0")
                        .text("response_format", "verbose_json")
                        .part("file", audio_part);
                    
                    match client.post(GROQ_WHISPER_URL)
                        .header("Authorization", format!("Bearer {}", groq_key))
                        .multipart(form)
                        .send()
                    {
                        Ok(resp) => {
                            if resp.status().is_success() {
                                match resp.json::<serde_json::Value>() {
                                    Ok(json) => {
                                        let text = json.get("text")
                                            .and_then(|t| t.as_str())
                                            .unwrap_or("")
                                            .to_string();
                                        tracing::info!("Transcription: {} chars", text.len());
                                        proxy_rpc.handle_response(id, Ok(ProxyResponse::AgentTranscription { text }));
                                    }
                                    Err(e) => {
                                        proxy_rpc.handle_response(id, Ok(ProxyResponse::AgentError {
                                            error: format!("Failed to parse Whisper response: {e}"),
                                        }));
                                    }
                                }
                            } else {
                                let status = resp.status();
                                let body = resp.text().unwrap_or_default();
                                proxy_rpc.handle_response(id, Ok(ProxyResponse::AgentError {
                                    error: format!("Whisper API error {status}: {body}"),
                                }));
                            }
                        }
                        Err(e) => {
                            proxy_rpc.handle_response(id, Ok(ProxyResponse::AgentError {
                                error: format!("Whisper request failed: {e}"),
                            }));
                        }
                    }
                });
            }
            AgentApproveToolCall { tool_call_id } => {
                tracing::info!("Agent tool call approved: {tool_call_id}");
                if let Some(sender) = self.pending_approvals.lock().remove(&tool_call_id) {
                    let _ = sender.send(true);
                }
                self.respond_rpc(id, Ok(ProxyResponse::AgentDone {
                    message: format!("Approved: {tool_call_id}"),
                }));
            }
            AgentRejectToolCall { tool_call_id } => {
                tracing::info!("Agent tool call rejected: {tool_call_id}");
                if let Some(sender) = self.pending_approvals.lock().remove(&tool_call_id) {
                    let _ = sender.send(false);
                }
                self.respond_rpc(id, Ok(ProxyResponse::AgentDone {
                    message: format!("Rejected: {tool_call_id}"),
                }));
            }

            // ── AI Diff Accept/Reject ─────────────────────────────
            AgentDiffAccept { diff_id, accepted_hunks } => {
                tracing::info!("Agent diff accepted: {diff_id}, hunks: {:?}", accepted_hunks);
                // The diff has already been applied to disk by the tool.
                // This acknowledges that the user wants to keep the changes.
                // In a future iteration, we could revert-then-selectively-apply hunks.
                self.respond_rpc(id, Ok(ProxyResponse::AgentDiffAcceptResponse {
                    diff_id,
                    success: true,
                    message: "Changes accepted".to_string(),
                }));
            }
            AgentDiffReject { diff_id } => {
                tracing::info!("Agent diff rejected: {diff_id}");
                // The user wants to revert the changes.
                // Look up the diff in our pending store and revert the file.
                if let Some(snapshot) = self.pending_diff_snapshots.lock().remove(&diff_id) {
                    let workspace = self.workspace.clone().unwrap_or_default();
                    let full_path = workspace.join(&snapshot.0);
                    match std::fs::write(&full_path, &snapshot.1) {
                        Ok(_) => {
                            tracing::info!("Reverted {} to pre-edit state", snapshot.0);
                            self.respond_rpc(id, Ok(ProxyResponse::AgentDiffRejectResponse { diff_id }));
                        }
                        Err(e) => {
                            self.respond_rpc(id, Err(RpcError {
                                code: 0,
                                message: format!("Failed to revert {}: {e}", snapshot.0),
                            }));
                        }
                    }
                } else {
                    self.respond_rpc(id, Ok(ProxyResponse::AgentDiffRejectResponse { diff_id }));
                }
            }
            AgentDiffAcceptAll {} => {
                tracing::info!("Agent diff accept all");
                self.pending_diff_snapshots.lock().clear();
                self.respond_rpc(id, Ok(ProxyResponse::AgentDiffAcceptResponse {
                    diff_id: "all".to_string(),
                    success: true,
                    message: "All changes accepted".to_string(),
                }));
            }
            AgentDiffRejectAll {} => {
                tracing::info!("Agent diff reject all");
                let snapshots: Vec<(String, String)> = {
                    let mut store = self.pending_diff_snapshots.lock();
                    let items: Vec<_> = store.drain().map(|(_, v)| v).collect();
                    items
                };
                let workspace = self.workspace.clone().unwrap_or_default();
                for (rel_path, old_content) in &snapshots {
                    let full_path = workspace.join(rel_path);
                    if let Err(e) = std::fs::write(&full_path, old_content) {
                        tracing::error!("Failed to revert {}: {e}", rel_path);
                    }
                }
                self.respond_rpc(id, Ok(ProxyResponse::AgentDiffRejectResponse {
                    diff_id: "all".to_string(),
                }));
            }

            // ── AI Inline Completion (ghost text) ────────────────
            AiInlineCompletion { request_id, path, position: _, prefix, suffix } => {
                tracing::debug!("AI inline completion request: path={}", path.display());
                let proxy_rpc = self.proxy_rpc.clone();
                let path = path.clone();
                let prefix = prefix.clone();
                let suffix = suffix.clone();
                let req_id = request_id;

                thread::spawn(move || {
                    let rt = match tokio::runtime::Builder::new_current_thread()
                        .enable_all()
                        .build()
                    {
                        Ok(rt) => rt,
                        Err(_) => {
                            proxy_rpc.handle_response(
                                id,
                                Ok(ProxyResponse::AiInlineCompletionResponse {
                                    request_id: req_id,
                                    items: vec![],
                                }),
                            );
                            return;
                        }
                    };

                    let items = rt.block_on(async {
                        crate::ai_completion::generate_completion(
                            &path, &prefix, &suffix,
                        )
                        .await
                    });

                    proxy_rpc.handle_response(
                        id,
                        Ok(ProxyResponse::AiInlineCompletionResponse {
                            request_id: req_id,
                            items,
                        }),
                    );
                });
            }

            // ── Code Index ─────────────────────────────────────────
            IndexWorkspace {} => {
                let workspace = self.workspace.clone();
                let core_rpc = self.core_rpc.clone();
                let proxy_rpc = self.proxy_rpc.clone();

                thread::spawn(move || {
                    let rt = match tokio::runtime::Runtime::new() {
                        Ok(rt) => rt,
                        Err(e) => {
                            core_rpc.notification(CoreNotification::IndexProgress {
                                status: format!("Error: {}", e),
                                progress: -1.0,
                            });
                            proxy_rpc.handle_response(id, Ok(ProxyResponse::IndexStarted {}));
                            return;
                        }
                    };

                    rt.block_on(async move {
                        let workspace_path = match workspace {
                            Some(p) => p,
                            None => {
                                core_rpc.notification(CoreNotification::IndexProgress {
                                    status: "No workspace open".to_string(),
                                    progress: -1.0,
                                });
                                proxy_rpc.handle_response(id, Ok(ProxyResponse::IndexStarted {}));
                                return;
                            }
                        };

                        let workspace_id = workspace_path
                            .file_name()
                            .and_then(|n| n.to_str())
                            .unwrap_or("default");

                        core_rpc.notification(CoreNotification::IndexProgress {
                            status: "Scanning files...".to_string(),
                            progress: 0.0,
                        });

                        let client = forge_agent::forge_search::client();
                        let core_rpc_clone = core_rpc.clone();

                        match client.scan_directory_with_progress(
                            workspace_id,
                            &workspace_path,
                            move |sent, total| {
                                let progress = if total > 0 {
                                    (sent as f64 / total as f64).min(0.99)
    } else {
                                    0.5
                                };
                                core_rpc_clone.notification(CoreNotification::IndexProgress {
                                    status: format!("Indexing {}/{} files...", sent, total),
                                    progress,
                                });
                            }
                        ).await {
                            Ok(result) => {
                                core_rpc.notification(CoreNotification::IndexProgress {
                                    status: format!(
                                        "{} symbols indexed",
                                        result.nodes_created
                                    ),
                                    progress: -1.0, // Done
                                });
                            }
                            Err(e) => {
                                core_rpc.notification(CoreNotification::IndexProgress {
                                    status: format!("Index error: {}", e),
                                    progress: -1.0,
                                });
                            }
                        }

                        proxy_rpc.handle_response(id, Ok(ProxyResponse::IndexStarted {}));
                    });
                });
            }

            IndexStatus {} => {
                let workspace = self.workspace.clone();
                let proxy_rpc = self.proxy_rpc.clone();

                thread::spawn(move || {
                    let rt = match tokio::runtime::Runtime::new() {
                        Ok(rt) => rt,
                        Err(_) => {
                            proxy_rpc.handle_response(
                                id,
                                Ok(ProxyResponse::IndexStatusResponse {
                                    is_indexed: false,
                                    symbol_count: 0,
                                }),
                            );
                            return;
                        }
                    };

                    rt.block_on(async move {
                        let workspace_id = workspace
                            .as_ref()
                            .and_then(|p| p.file_name())
                            .and_then(|n| n.to_str())
                            .unwrap_or("default");

                        let client = forge_agent::forge_search::client();

                        let (is_indexed, symbol_count) = client
                            .check_index_status(workspace_id)
                            .await
                            .unwrap_or((false, 0));

                        proxy_rpc.handle_response(
                            id,
                            Ok(ProxyResponse::IndexStatusResponse {
                                is_indexed,
                                symbol_count,
                            }),
                        );
                    });
                });
            }

            // ── LSP Tools for AI Agent ────────────────────────────
            LspGotoDefinition { path, position } => {
                let proxy_rpc = self.proxy_rpc.clone();
                self.catalog_rpc.get_definition(
                    &path,
                    position,
                    move |_, result| {
                        let locations = match result {
                            Ok(response) => match response {
                                lsp_types::GotoDefinitionResponse::Scalar(loc) => vec![loc],
                                lsp_types::GotoDefinitionResponse::Array(locs) => locs,
                                lsp_types::GotoDefinitionResponse::Link(links) => {
                                    links.into_iter().map(|l| lsp_types::Location {
                                        uri: l.target_uri,
                                        range: l.target_selection_range,
                                    }).collect()
                                }
                            },
                            Err(_) => vec![],
                        };
                        proxy_rpc.handle_response(
                            id,
                            Ok(ProxyResponse::LspGotoDefinitionResponse { locations }),
                        );
                    },
                );
            }

            LspFindReferences { path, position, include_declaration } => {
                let proxy_rpc = self.proxy_rpc.clone();
                // Note: include_declaration is passed but the underlying API may not use it
                let _ = include_declaration;
                self.catalog_rpc.get_references(
                    &path,
                    position,
                    move |_, result| {
                        let locations = result.unwrap_or_default();
                        proxy_rpc.handle_response(
                            id,
                            Ok(ProxyResponse::LspFindReferencesResponse { locations }),
                        );
                    },
                );
            }

            LspHover { path, position } => {
                let proxy_rpc = self.proxy_rpc.clone();
                self.catalog_rpc.hover(&path, position, move |_, result| {
                    let (contents, range) = match result {
                        Ok(hover) => {
                            let text = match hover.contents {
                                lsp_types::HoverContents::Scalar(content) => {
                                    match content {
                                        lsp_types::MarkedString::String(s) => s,
                                        lsp_types::MarkedString::LanguageString(ls) => {
                                            format!("```{}\n{}\n```", ls.language, ls.value)
                                        }
                                    }
                                }
                                lsp_types::HoverContents::Array(arr) => {
                                    arr.into_iter().map(|c| match c {
                                        lsp_types::MarkedString::String(s) => s,
                                        lsp_types::MarkedString::LanguageString(ls) => {
                                            format!("```{}\n{}\n```", ls.language, ls.value)
                                        }
                                    }).collect::<Vec<_>>().join("\n\n")
                                }
                                lsp_types::HoverContents::Markup(markup) => markup.value,
                            };
                            (Some(text), hover.range)
                        }
                        Err(_) => (None, None),
                    };
                    proxy_rpc.handle_response(
                        id,
                        Ok(ProxyResponse::LspHoverResponse { contents, range }),
                    );
                });
            }

            LspGetDiagnostics { path: _ } => {
                // Diagnostics are pushed via CoreNotification::PublishDiagnostics
                // and cached in the UI layer (lapce-app). For now, return empty.
                // TODO: Add a diagnostics cache to catalog_rpc if needed.
                self.proxy_rpc.handle_response(
                    id,
                    Ok(ProxyResponse::LspDiagnosticsResponse { diagnostics: vec![] }),
                );
            }

            LspPrepareRename { path, position } => {
                let proxy_rpc = self.proxy_rpc.clone();
                self.catalog_rpc.prepare_rename(
                    &path,
                    position,
                    move |_, result| {
                        let (range, placeholder) = match result {
                            Ok(response) => match response {
                                lsp_types::PrepareRenameResponse::Range(r) => (Some(r), None),
                                lsp_types::PrepareRenameResponse::RangeWithPlaceholder { range, placeholder } => {
                                    (Some(range), Some(placeholder))
                                }
                                lsp_types::PrepareRenameResponse::DefaultBehavior { .. } => (None, None),
                            },
                            Err(_) => (None, None),
                        };
                        proxy_rpc.handle_response(
                            id,
                            Ok(ProxyResponse::LspPrepareRenameResponse { range, placeholder }),
                        );
                    },
                );
            }

            LspRename { path, position, new_name } => {
                let proxy_rpc = self.proxy_rpc.clone();
                self.catalog_rpc.rename(
                    &path,
                    position,
                    new_name,
                    move |_, result| {
                        let edit = result.ok();
                        proxy_rpc.handle_response(
                            id,
                            Ok(ProxyResponse::LspRenameResponse { edit }),
                        );
                    },
                );
            }
        }
    }
}

// ══════════════════════════════════════════════════════════════════
//  XML Tool Call Parser
//  Handles LLMs that emit tool calls as XML tags in their text output
// (XML tool parsing code removed — all LLM calls now go through forge-search)
// ══════════════════════════════════════════════════════════════════

/// Execute an IDE tool locally (the "Hands" executing what the "Brain" requested).
/// This handles tool calls that need to run in the IDE context.
///
/// `execute_command` and `execute_background` are routed through the IDE's real
/// terminal (PTY) so the user can see the output and the shell profile is loaded.
async fn execute_ide_tool(
    tc: &forge_agent::ToolCallInfo,
    workspace_path: &std::path::Path,
    core_rpc: &CoreRpcHandler,
    agent_term_mgr: &AgentTerminalManager,
    ide_terminals: &Arc<std::sync::Mutex<HashMap<TermId, TerminalSender>>>,
) -> forge_agent::tools::ToolResult {
    match tc.name.as_str() {
        // ── Command execution: use real IDE terminal ──────────────
        "execute_command" => {
            let command = tc.args.get("command")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            if command.is_empty() {
                return forge_agent::tools::ToolResult::err("Missing 'command' parameter");
            }
            let timeout_secs = tc.args.get("timeout_secs")
                .and_then(|v| v.as_u64())
                .unwrap_or(120)
                .min(600);

            agent_term_mgr.execute_command(
                command, workspace_path, timeout_secs, core_rpc, ide_terminals,
            )
        }
        "execute_background" => {
            let command = tc.args.get("command")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            if command.is_empty() {
                return forge_agent::tools::ToolResult::err("Missing 'command' parameter");
            }
            let wait_seconds = tc.args.get("wait_seconds")
                .and_then(|v| v.as_u64())
                .unwrap_or(3);

            agent_term_mgr.execute_background(
                command, workspace_path, wait_seconds, core_rpc, ide_terminals,
            )
        }
        "read_process_output" => {
            let pid = tc.args.get("pid").and_then(|v| v.as_u64()).map(|p| p as u32);
            if let Some(pid) = pid {
                // Check if this PID belongs to an agent terminal
                if agent_term_mgr.has_terminal(pid) {
                    let tail_lines = tc.args.get("tail_lines")
                        .and_then(|v| v.as_u64())
                        .unwrap_or(100) as usize;
                    if let Some(output) = agent_term_mgr.read_output(pid, tail_lines) {
                        let is_running = agent_term_mgr.is_running(pid);
                        return forge_agent::tools::ToolResult::ok(format!(
                            "Running: {is_running}\n--- Output (tail {tail_lines} lines) ---\n{output}"
                        ));
                    }
                }
            }
            // Fall through to default handler for non-agent-terminal PIDs
            let tool_call_obj = forge_agent::tools::ToolCall {
                name: tc.name.clone(),
                arguments: tc.args.clone(),
                thought_signature: None,
            };
            forge_agent::tools::execute(&tool_call_obj, workspace_path, false).await
        }
        // ── LSP tools: not yet available ─────────────────────────
        "lsp_go_to_definition" | "lsp_find_references" | "lsp_hover" | "lsp_rename" => {
            forge_agent::tools::ToolResult::err(format!(
                "LSP tool '{}' is not yet available in cloud mode. \
                Use 'codebase_search' for semantic search or \
                'trace_call_chain' to find callers/callees. \
                These use the pre-indexed code graph and are often faster.",
                tc.name
            ))
        }
        // ── All other tools: use standard execution ──────────────
        _ => {
            let tool_call_obj = forge_agent::tools::ToolCall {
                name: tc.name.clone(),
                arguments: tc.args.clone(),
                thought_signature: None,
            };
            forge_agent::tools::execute(&tool_call_obj, workspace_path, false).await
        }
    }
}

/// Collect relevant files from the workspace to attach to the AI prompt.
/// This gives the cloud "Brain" live context about what the user is working on.
fn collect_relevant_files(workspace_path: &Path) -> Vec<serde_json::Value> {
    let mut files = Vec::new();
    
    // For now, we'll just collect a few key files if they exist.
    // In a more sophisticated implementation, we could:
    // 1. Include currently open files from the editor state
    // 2. Include recently modified files
    // 3. Include files mentioned in the prompt
    
    // Common config/entry files
    let key_files = [
        "Cargo.toml",
        "package.json", 
        "pyproject.toml",
        "go.mod",
        "src/main.rs",
        "src/lib.rs",
        "app/main.py",
        "index.ts",
    ];
    
    for name in key_files {
        let path = workspace_path.join(name);
        if path.exists() {
            if let Ok(content) = std::fs::read_to_string(&path) {
                // Limit file size to avoid token explosion
                let truncated = if content.len() > 4000 {
                    format!("{}...(truncated)", &content[..4000])
            } else {
                    content
                };
                
                files.push(serde_json::json!({
                    "path": name,
                    "content": truncated,
                }));
                
                // Limit to 3 files to avoid too much context
                if files.len() >= 3 {
                break;
            }
        }
        }
    }
    
    files
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
            terminals: Arc::new(std::sync::Mutex::new(HashMap::new())),
            file_watcher,
            window_id: 1,
            tab_id: 1,
            db_manager: crate::database::connection_manager::ConnectionManager::new(),
            pending_diff_snapshots: Arc::new(Mutex::new(HashMap::new())),
            pending_approvals: Arc::new(Mutex::new(HashMap::new())),
            agent_terminal_mgr: Arc::new(AgentTerminalManager::new()),
        }
    }

    fn respond_rpc(&self, id: RequestId, result: Result<ProxyResponse, RpcError>) {
        self.proxy_rpc.handle_response(id, result);
    }

    /// Resolve a potentially relative path to an absolute one using the workspace root.
    /// This prevents panics in `Url::from_file_path()` which fails on relative paths.
    fn resolve_path(&self, path: PathBuf) -> PathBuf {
        if path.is_absolute() {
            path
        } else if let Some(workspace) = &self.workspace {
            workspace.join(&path)
        } else {
            // Last resort: use current directory
            std::env::current_dir().unwrap_or_default().join(&path)
        }
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
    // Use gix to check if repo exists (faster, pure Rust)
    if !crate::gix_utils::is_git_repo(workspace_path) {
        tracing::info!("[git_init] Initializing new repository at {:?}", workspace_path);
        crate::gix_utils::init_repo(workspace_path)?;
    } else {
        tracing::info!("[git_init] Repository already exists at {:?}", workspace_path);
    }
    Ok(())
}

fn git_commit(
    workspace_path: &Path,
    message: &str,
    diffs: Vec<FileDiff>,
) -> Result<()> {
    // Stage files based on diffs
    let mut to_add: Vec<PathBuf> = Vec::new();
    let mut to_remove: Vec<PathBuf> = Vec::new();
    
    for diff in diffs {
        match diff {
            FileDiff::Modified(p) | FileDiff::Added(p) => {
                if let Ok(rel) = p.strip_prefix(workspace_path) {
                    to_add.push(rel.to_path_buf());
                } else {
                    to_add.push(p);
                }
            }
            FileDiff::Renamed(a, d) => {
                if let Ok(rel) = a.strip_prefix(workspace_path) {
                    to_add.push(rel.to_path_buf());
                } else {
                    to_add.push(a);
                }
                if let Ok(rel) = d.strip_prefix(workspace_path) {
                    to_remove.push(rel.to_path_buf());
                } else {
                    to_remove.push(d);
                }
            }
            FileDiff::Deleted(p) => {
                if let Ok(rel) = p.strip_prefix(workspace_path) {
                    to_remove.push(rel.to_path_buf());
                } else {
                    to_remove.push(p);
                }
            }
        }
    }
    
    // Stage files using gix_utils
    if !to_add.is_empty() {
        crate::gix_utils::stage_files(workspace_path, &to_add)?;
    }
    
    // Handle removed files
    if !to_remove.is_empty() {
        use std::process::Command;
        let path_strs: Vec<&str> = to_remove.iter()
            .filter_map(|p| p.to_str())
            .collect();
        let _ = Command::new("git")
            .args(["rm", "--cached", "--ignore-unmatch", "--"])
            .args(&path_strs)
            .current_dir(workspace_path)
            .output();
    }
    
    // Commit using gix_utils
    crate::gix_utils::commit(workspace_path, message)?;
            Ok(())
        }

use lapce_rpc::source_control::{GitCheckoutResult, GitCheckoutStatus};

/// Attempt checkout without force - returns conflict status if there are local changes
fn git_checkout_with_status(workspace_path: &Path, reference: &str) -> Result<GitCheckoutResult> {
    tracing::info!("[GIT CHECKOUT] Attempting checkout for: {} in {:?}", reference, workspace_path);
    
    // Early validation using gix (faster, pure Rust)
    if let Err(e) = crate::gix_utils::validate_checkout_target(workspace_path, reference) {
        tracing::error!("[GIT CHECKOUT] Invalid target: {}", e);
        return Ok(GitCheckoutResult {
            status: GitCheckoutStatus::Error,
            message: format!("Cannot find '{}': {}", reference, e),
            checked_out_ref: None,
        });
    }
    
    // Check if there are uncommitted changes
    let has_changes = crate::gix_utils::has_uncommitted_changes(workspace_path).unwrap_or(false);
    
    if has_changes {
        tracing::info!("[GIT CHECKOUT] Conflict detected: uncommitted changes present");
        return Ok(GitCheckoutResult {
            status: GitCheckoutStatus::Conflict,
            message: format!("Cannot checkout '{}': local changes would be overwritten. Choose 'Smart Checkout' to stash changes or 'Force Checkout' to discard them.", reference),
            checked_out_ref: None,
        });
    }
    
    // Try checkout without force
    match crate::gix_utils::checkout(workspace_path, reference, false) {
        Ok(()) => {
            tracing::info!("[GIT CHECKOUT] Success: checked out {}", reference);
            Ok(GitCheckoutResult {
                status: GitCheckoutStatus::Success,
                message: format!("Successfully checked out '{}'", reference),
                checked_out_ref: Some(reference.to_string()),
            })
        }
        Err(e) => {
            let err_msg = e.to_string();
            if err_msg.contains("conflict") || err_msg.contains("overwritten") {
                tracing::info!("[GIT CHECKOUT] Conflict detected: {}", err_msg);
                Ok(GitCheckoutResult {
                    status: GitCheckoutStatus::Conflict,
                    message: format!("Cannot checkout '{}': local changes would be overwritten. Choose 'Smart Checkout' to stash changes or 'Force Checkout' to discard them.", reference),
                    checked_out_ref: None,
                })
            } else {
                tracing::error!("[GIT CHECKOUT] Error: {:?}", e);
                Ok(GitCheckoutResult {
                    status: GitCheckoutStatus::Error,
                    message: format!("Checkout failed: {}", err_msg),
                    checked_out_ref: None,
                })
            }
        }
    }
}

/// Smart checkout: stash -> checkout -> stash pop
fn git_smart_checkout(workspace_path: &Path, reference: &str) -> Result<GitCheckoutResult> {
    tracing::info!("[GIT SMART CHECKOUT] Starting for: {} in {:?}", reference, workspace_path);
    
    // Validate checkout target using gix first (fast validation)
    if let Err(e) = crate::gix_utils::validate_checkout_target(workspace_path, reference) {
        tracing::error!("[GIT SMART CHECKOUT] Invalid checkout target: {}", e);
        return Ok(GitCheckoutResult {
            status: GitCheckoutStatus::Error,
            message: format!("Invalid checkout target: {}", e),
            checked_out_ref: None,
        });
    }
    
    // Check if there are changes to stash using gix
    let has_changes = crate::gix_utils::has_uncommitted_changes(workspace_path).unwrap_or(false);
    
    let stashed = if has_changes {
        // Create stash using gix_utils
        match crate::gix_utils::stash_save(workspace_path, Some("Auto-stash before checkout"), true) {
            Ok(_) => {
                tracing::info!("[GIT SMART CHECKOUT] Changes stashed successfully");
                true
            }
            Err(e) => {
                tracing::error!("[GIT SMART CHECKOUT] Failed to stash: {:?}", e);
                return Ok(GitCheckoutResult {
                    status: GitCheckoutStatus::Error,
                    message: format!("Failed to stash changes: {}", e),
                    checked_out_ref: None,
                });
            }
        }
    } else {
        false
    };
    
    // Now perform checkout
    let checkout_result = crate::gix_utils::checkout(workspace_path, reference, false);
    
    // Handle checkout failure
    if let Err(e) = checkout_result {
        // If checkout fails, try to restore stash
        if stashed {
            let _ = crate::gix_utils::stash_pop(workspace_path, 0);
        }
        return Ok(GitCheckoutResult {
            status: GitCheckoutStatus::Error,
            message: format!("Checkout failed: {}", e),
            checked_out_ref: None,
        });
    }
    
    // Pop the stash
    if stashed {
        match crate::gix_utils::stash_pop(workspace_path, 0) {
            Ok(()) => {
                tracing::info!("[GIT SMART CHECKOUT] Stash popped successfully");
                Ok(GitCheckoutResult {
                    status: GitCheckoutStatus::Success,
                    message: format!("Successfully checked out '{}' and restored local changes", reference),
                    checked_out_ref: Some(reference.to_string()),
                })
            }
            Err(e) => {
                tracing::warn!("[GIT SMART CHECKOUT] Stash pop had conflicts: {:?}", e);
                Ok(GitCheckoutResult {
                    status: GitCheckoutStatus::Success,
                    message: format!("Checked out '{}'. Note: Restoring stashed changes had conflicts - your changes are still in stash.", reference),
                    checked_out_ref: Some(reference.to_string()),
                })
            }
        }
    } else {
        Ok(GitCheckoutResult {
            status: GitCheckoutStatus::Success,
            message: format!("Successfully checked out '{}'", reference),
            checked_out_ref: Some(reference.to_string()),
        })
    }
}

/// Force checkout: discard local changes
fn git_force_checkout(workspace_path: &Path, reference: &str) -> Result<GitCheckoutResult> {
    tracing::info!("[GIT FORCE CHECKOUT] Starting for: {} in {:?}", reference, workspace_path);
    
    // Use gix_utils checkout with force=true
    match crate::gix_utils::checkout(workspace_path, reference, true) {
        Ok(()) => {
            tracing::info!("[GIT FORCE CHECKOUT] Success");
            Ok(GitCheckoutResult {
                status: GitCheckoutStatus::Success,
                message: format!("Successfully checked out '{}' (local changes discarded)", reference),
                checked_out_ref: Some(reference.to_string()),
            })
        }
        Err(e) => {
            tracing::error!("[GIT FORCE CHECKOUT] Error: {:?}", e);
            Ok(GitCheckoutResult {
                status: GitCheckoutStatus::Error,
                message: format!("Force checkout failed: {}", e),
                checked_out_ref: None,
            })
        }
    }
}

/// Legacy checkout function (kept for notification handler)
fn git_checkout(workspace_path: &Path, reference: &str) -> Result<()> {
    let result = git_force_checkout(workspace_path, reference)?;
    if result.status == GitCheckoutStatus::Success {
    Ok(())
    } else {
        Err(anyhow::anyhow!(result.message))
    }
}

fn git_discard_files_changes<'a>(
    workspace_path: &Path,
    files: impl Iterator<Item = &'a Path>,
) -> Result<()> {
    let paths: Vec<PathBuf> = files
        .filter_map(|path| {
            path.strip_prefix(workspace_path)
                .ok()
                .map(|p| p.to_path_buf())
        })
        .collect();
    
    if paths.is_empty() {
        // If there are no paths then we do nothing
        return Ok(());
    }
    
    crate::gix_utils::discard_file_changes(workspace_path, &paths)
}

fn git_discard_workspace_changes(workspace_path: &Path) -> Result<()> {
    crate::gix_utils::discard_all_changes(workspace_path)
}

fn git_diff_new(workspace_path: &Path) -> Option<DiffInfo> {
    use std::process::Command;
    
    // Get current branch name
    let name = crate::gix_utils::get_head_name_from_path(workspace_path).ok()?.unwrap_or_else(|| "(No branch)".to_string());
    
    // Get branches
    let branches = crate::gix_utils::list_branches(workspace_path, true)
        .ok()?
        .into_iter()
        .map(|b| b.name)
        .collect();
    
    // Get tags
    let tags = crate::gix_utils::list_tags(workspace_path).ok()?;
    
    // Get status using git command
    let output = Command::new("git")
        .args(["status", "--porcelain=v1", "-uall"])
        .current_dir(workspace_path)
        .output()
        .ok()?;
    
    let stdout = String::from_utf8_lossy(&output.stdout);
    
    // Separate lists for staged, unstaged, and untracked
    let mut staged = Vec::new();
    let mut unstaged = Vec::new();
    let mut untracked = Vec::new();
    let mut all_diffs = Vec::new(); // Combined for backwards compatibility
    
    for line in stdout.lines() {
        if line.len() < 3 {
            continue;
        }
        let status = &line[0..2];
        let path = line[3..].trim();
        
        // Handle renames (R status shows "old -> new")
        if status.starts_with('R') {
            let parts: Vec<&str> = path.split(" -> ").collect();
            if parts.len() == 2 {
                let diff = FileDiff::Renamed(
                    workspace_path.join(parts[0]),
                    workspace_path.join(parts[1]),
                );
                // Renames in index are staged
                staged.push(diff.clone());
                all_diffs.push(diff);
            }
            continue;
        }
        
        let full_path = workspace_path.join(path);
        
        // Parse status codes
        // First char is index (staged) status, second is worktree (unstaged) status
        // ' ' = unmodified, M = modified, A = added, D = deleted, R = renamed
        // C = copied, U = updated but unmerged, ? = untracked, ! = ignored
        let index_status = status.chars().next().unwrap_or(' ');
        let worktree_status = status.chars().nth(1).unwrap_or(' ');
        
        match (index_status, worktree_status) {
            // Untracked files - only add to untracked list, NOT to all_diffs
            ('?', '?') => {
                untracked.push(full_path);
            }
            // Ignored files - skip
            ('!', '!') => {}
            // Staged changes (index has changes)
            _ => {
                // Check staged (index) status
                match index_status {
                    'M' => {
                        staged.push(FileDiff::Modified(full_path.clone()));
                    }
                    'A' => {
                        staged.push(FileDiff::Added(full_path.clone()));
                    }
                    'D' => {
                        staged.push(FileDiff::Deleted(full_path.clone()));
                    }
                    _ => {}
                }
                
                // Check unstaged (worktree) status
                match worktree_status {
                    'M' => {
                        unstaged.push(FileDiff::Modified(full_path.clone()));
                    }
                    'D' => {
                        unstaged.push(FileDiff::Deleted(full_path.clone()));
                    }
                    _ => {}
                }
                
                // Add to combined list if anything changed
                if index_status != ' ' || worktree_status != ' ' {
                    // Use the most significant change for combined view
                    let combined_diff = match (index_status, worktree_status) {
                        ('D', _) | (_, 'D') => FileDiff::Deleted(full_path),
                        ('A', _) => FileDiff::Added(full_path),
                        ('M', _) | (_, 'M') => FileDiff::Modified(full_path),
                        _ => continue,
                    };
                    all_diffs.push(combined_diff);
                }
            }
        }
    }
    
    // Sort all lists by path
    let sort_fn = |d: &FileDiff| d.path().clone();
    staged.sort_by_key(sort_fn);
    unstaged.sort_by_key(sort_fn);
    untracked.sort();
    all_diffs.sort_by_key(sort_fn);
    
    Some(DiffInfo {
        head: name,
        branches,
        tags,
        diffs: all_diffs,
        staged,
        unstaged,
        untracked,
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
    use std::process::Command;
    
    // Get HEAD commit id
    let head_id = crate::gix_utils::get_head_commit_id(workspace_path)
        .ok()
        .flatten()
        .map(|id| id.to_string());
    
    // Build git log command with custom format
    // Format: hash|short_hash|author_name|author_email|timestamp|parent_hashes|summary|message
    // The -z flag makes git separate records with NUL bytes
    let format = "%H|%h|%an|%ae|%at|%P|%s|%B";
    
    let mut args = vec![
        "log".to_string(),
        format!("--format={}", format),
        format!("-n{}", limit + skip), // Get extra to handle skip
        "-z".to_string(), // Use NUL as record separator between commits
    ];
    
    if let Some(ref author_filter) = author {
        args.push(format!("--author={}", author_filter));
    }
    
    if let Some(ref search_text) = search {
        args.push(format!("--grep={}", search_text));
        args.push("-i".to_string()); // Case insensitive
    }
    
    // Add branch or default to HEAD
    if let Some(ref branch_name) = branch {
        args.push(branch_name.clone());
    }
    
    let output = Command::new("git")
        .args(&args)
        .current_dir(workspace_path)
        .output()
        .map_err(|e| {
            eprintln!("[GIT_LOG] Command::new(\"git\") failed: {:?}", e);
            eprintln!("[GIT_LOG] PATH: {:?}", std::env::var("PATH"));
            anyhow::anyhow!("Failed to run git log: {}", e)
        })?;
    
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("git log failed: {}", stderr);
    }
    
    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut commits = Vec::new();
    let total_count;
    
    // Get all branches for annotation
    let branches_list = crate::gix_utils::list_branches(workspace_path, true).unwrap_or_default();
    let mut branch_map: std::collections::HashMap<String, Vec<String>> = std::collections::HashMap::new();
    for b in branches_list {
        if let Some(ref commit_id) = b.last_commit_id {
            branch_map.entry(commit_id.clone()).or_default().push(b.name);
        }
    }
    
    // Get all tags for annotation
    let tags_list = crate::gix_utils::list_tags(workspace_path).unwrap_or_default();
    let tag_map: std::collections::HashMap<String, Vec<String>> = std::collections::HashMap::new();
    // Note: For now, we don't have commit IDs for tags, so this stays empty
    let _ = tags_list; // Suppress unused warning
    
    // Parse commits
    let records: Vec<&str> = stdout.split('\0').filter(|s| !s.trim().is_empty()).collect();
    total_count = records.len();
    
    for (idx, record) in records.iter().enumerate() {
        if idx < skip {
            continue;
        }
        if commits.len() >= limit {
            break;
        }
        
        let parts: Vec<&str> = record.splitn(8, '|').collect();
        if parts.len() < 7 {
            continue;
        }
        
        let id = parts[0].to_string();
        let short_id = parts[1].to_string();
        let author_name = parts[2].to_string();
        let author_email = parts[3].to_string();
        let timestamp: i64 = parts[4].parse().unwrap_or(0);
        let parents: Vec<String> = parts[5].split_whitespace().map(|s| s.to_string()).collect();
        let summary = parts[6].to_string();
        let message = if parts.len() > 7 { parts[7].to_string() } else { summary.clone() };
        
        let is_head = head_id.as_ref() == Some(&id);
        let commit_branches = branch_map.get(&id).cloned().unwrap_or_default();
        let commit_tags = tag_map.get(&id).cloned().unwrap_or_default();
        
        commits.push(GitCommitInfo {
            id,
            short_id,
            summary,
            message,
            author_name,
            author_email,
            timestamp,
            parents,
            branches: commit_branches,
            tags: commit_tags,
            is_head,
        });
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
    use std::process::Command;
    
    // Use gix_utils for basic branch listing
    let mut branches = crate::gix_utils::list_branches(workspace_path, include_remote)?;
    tracing::info!("[gix] Successfully listed {} branches", branches.len());
    
    // Enhance with upstream tracking info using git command
    for branch in &mut branches {
        if !branch.is_remote {
            // Get upstream branch name
            if let Ok(output) = Command::new("git")
                .args(["config", "--get", &format!("branch.{}.remote", branch.name)])
                .current_dir(workspace_path)
                .output()
            {
                if output.status.success() {
                    let remote = String::from_utf8_lossy(&output.stdout).trim().to_string();
                    if let Ok(merge_output) = Command::new("git")
                        .args(["config", "--get", &format!("branch.{}.merge", branch.name)])
                        .current_dir(workspace_path)
                        .output()
                    {
                        if merge_output.status.success() {
                            let merge = String::from_utf8_lossy(&merge_output.stdout).trim().to_string();
                            let upstream_branch = merge.strip_prefix("refs/heads/").unwrap_or(&merge);
                            branch.upstream = Some(format!("{}/{}", remote, upstream_branch));
                            
                            // Get ahead/behind counts
                            if let Ok(count_output) = Command::new("git")
                                .args(["rev-list", "--left-right", "--count", 
                                       &format!("{}...{}/{}", branch.name, remote, upstream_branch)])
                                .current_dir(workspace_path)
                                .output()
                            {
                                if count_output.status.success() {
                                    let counts = String::from_utf8_lossy(&count_output.stdout);
                                    let parts: Vec<&str> = counts.trim().split('\t').collect();
                                    if parts.len() == 2 {
                                        branch.ahead = parts[0].parse().unwrap_or(0);
                                        branch.behind = parts[1].parse().unwrap_or(0);
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
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
    
    crate::gix_utils::create_branch(workspace_path, name, start_point)?;
    
    // Checkout if requested
    if checkout {
        crate::gix_utils::checkout(workspace_path, name, false)?;
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
    
    crate::gix_utils::delete_branch(workspace_path, name, force)?;
    
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
    
    crate::gix_utils::rename_branch(workspace_path, old_name, new_name)?;
    
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
    
    let gix_entries = crate::gix_utils::stash_list(workspace_path)?;
    let entries: Vec<GitStashEntry> = gix_entries
        .into_iter()
        .map(|e| GitStashEntry {
            index: e.index,
            message: e.message,
            branch: e.branch,
            commit_id: e.commit_id,
            timestamp: e.timestamp,
        })
        .collect();
    
    Ok(GitStashList { entries })
}

fn git_stash_save(
    workspace_path: &Path,
    message: Option<&str>,
    include_untracked: bool,
    _keep_index: bool,
) -> Result<lapce_rpc::source_control::GitStashResult> {
    use lapce_rpc::source_control::GitStashResult;
    
    crate::gix_utils::stash_save(workspace_path, message, include_untracked)?;
    
    Ok(GitStashResult {
        success: true,
        message: "Stash created".to_string(),
    })
}

fn git_stash_pop(workspace_path: &Path, index: usize) -> Result<lapce_rpc::source_control::GitStashResult> {
    use lapce_rpc::source_control::GitStashResult;
    
    crate::gix_utils::stash_pop(workspace_path, index)?;
    
    Ok(GitStashResult {
        success: true,
        message: "Stash applied and dropped".to_string(),
    })
}

fn git_stash_apply(workspace_path: &Path, index: usize) -> Result<lapce_rpc::source_control::GitStashResult> {
    use lapce_rpc::source_control::GitStashResult;
    
    crate::gix_utils::stash_apply(workspace_path, index)?;
    
    Ok(GitStashResult {
        success: true,
        message: "Stash applied".to_string(),
    })
}

fn git_stash_drop(workspace_path: &Path, index: usize) -> Result<lapce_rpc::source_control::GitStashResult> {
    use lapce_rpc::source_control::GitStashResult;
    
    crate::gix_utils::stash_drop(workspace_path, index)?;
    
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
    
    let result = crate::gix_utils::merge(
        workspace_path,
        &options.branch,
        options.message.as_deref(),
        options.no_ff,
        options.squash,
    )?;
    
    Ok(GitMergeResult {
        success: result.success,
        message: result.message,
        conflicts: result.conflicts,
        merged_commit: result.merged_commit,
    })
}

fn git_merge_abort(workspace_path: &Path) -> Result<lapce_rpc::source_control::GitMergeResult> {
    use lapce_rpc::source_control::GitMergeResult;
    
    crate::gix_utils::merge_abort(workspace_path)?;
    
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
    
    let mode = match options.mode {
        GitResetMode::Soft => crate::gix_utils::ResetMode::Soft,
        GitResetMode::Mixed => crate::gix_utils::ResetMode::Mixed,
        GitResetMode::Hard => crate::gix_utils::ResetMode::Hard,
        GitResetMode::Keep => crate::gix_utils::ResetMode::Keep,
    };
    
    crate::gix_utils::reset(workspace_path, &options.target, mode)?;
    
    let new_head = crate::gix_utils::get_head_commit_id(workspace_path)
        .ok()
        .flatten()
        .map(|id| id.to_string());
    
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
    
    let relative_path = path.strip_prefix(workspace_path).unwrap_or(path);
    let blame_entries = crate::gix_utils::blame(workspace_path, relative_path, commit)?;
    
    let lines: Vec<GitBlameLine> = blame_entries
        .into_iter()
        .enumerate()
        .map(|(idx, entry)| GitBlameLine {
            line_number: idx + 1,
            commit_id: entry.commit_id.clone(),
            short_commit_id: entry.commit_id.chars().take(7).collect(),
            author_name: entry.author_name,
            author_email: entry.author_email,
            timestamp: entry.timestamp,
            summary: entry.summary,
            original_line_number: entry.original_line,
            original_path: entry.original_path,
        })
        .collect();
    
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
    
    // Use gix_utils to list tags
    let tag_names = crate::gix_utils::list_tags(workspace_path)?;
    
    // For now, return basic tag info (tag details need to be fetched separately if needed)
    let tags: Vec<GitTagInfo> = tag_names
        .into_iter()
        .map(|name| GitTagInfo {
            name,
            commit_id: String::new(),
            message: None,
            tagger_name: None,
            tagger_email: None,
            timestamp: None,
            is_annotated: false,
        })
        .collect();
    
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
    
    // Note: annotated tags are created when message is provided
crate::gix_utils::create_tag(workspace_path, name, target, if annotated { message } else { None })?;
    
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
    
    crate::gix_utils::delete_tag(workspace_path, name)?;
    
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
    
    let remote_names = crate::gix_utils::list_remotes(workspace_path)?;
    let mut remotes = Vec::new();
    
    for name in remote_names {
        let url = crate::gix_utils::get_remote_url(workspace_path, &name)
            .ok()
            .flatten()
            .unwrap_or_default();
        remotes.push(GitRemoteInfo {
            name,
            fetch_url: url.clone(),
            push_url: url,
        });
    }
    
    Ok(GitRemoteList { remotes })
}

fn git_add_remote(
    workspace_path: &Path,
    name: &str,
    url: &str,
) -> Result<lapce_rpc::source_control::GitRemoteResult> {
    use lapce_rpc::source_control::GitRemoteResult;
    
    crate::gix_utils::add_remote(workspace_path, name, url)?;
    
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
    
    crate::gix_utils::remove_remote(workspace_path, name)?;
    
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
    use std::process::Command;
    
    // Check repository state using git command
    let git_dir = crate::gix_utils::get_git_dir(workspace_path)?;
    
    let is_rebasing = git_dir.join("rebase-merge").exists() || git_dir.join("rebase-apply").exists();
    let is_merging = git_dir.join("MERGE_HEAD").exists();
    let is_cherry_picking = git_dir.join("CHERRY_PICK_HEAD").exists();
    let is_reverting = git_dir.join("REVERT_HEAD").exists();
    let is_bisecting = git_dir.join("BISECT_LOG").exists();
    
    // Get conflicts using git status
    let mut conflicts = Vec::new();
    if let Ok(output) = Command::new("git")
        .args(["status", "--porcelain=v1"])
        .current_dir(workspace_path)
        .output()
    {
        let stdout = String::from_utf8_lossy(&output.stdout);
        for line in stdout.lines() {
            if line.starts_with("UU ") || line.starts_with("AA ") || line.starts_with("DD ") {
                let path = line[3..].trim();
                conflicts.push(PathBuf::from(path));
            }
        }
    }
    
    Ok(GitStatus {
        is_rebasing,
        is_merging,
        is_cherry_picking,
        is_reverting,
        is_bisecting,
        rebase_head_name: None,
        merge_head: None,
        conflicts,
    })
}

// ============================================================================
// Git Stage Operations
// ============================================================================

fn git_stage_files(workspace_path: &Path, paths: &[PathBuf]) -> Result<()> {
    let relative_paths: Vec<PathBuf> = paths
        .iter()
        .map(|p| p.strip_prefix(workspace_path).unwrap_or(p).to_path_buf())
        .collect();
    crate::gix_utils::stage_files(workspace_path, &relative_paths)
}

fn git_unstage_files(workspace_path: &Path, paths: &[PathBuf]) -> Result<()> {
    let relative_paths: Vec<PathBuf> = paths
        .iter()
        .map(|p| p.strip_prefix(workspace_path).unwrap_or(p).to_path_buf())
        .collect();
    crate::gix_utils::unstage_files(workspace_path, &relative_paths)
}

fn git_stage_all(workspace_path: &Path) -> Result<()> {
    crate::gix_utils::stage_all(workspace_path)
}

fn git_unstage_all(workspace_path: &Path) -> Result<()> {
    crate::gix_utils::unstage_all(workspace_path)
}

fn file_get_head(workspace_path: &Path, path: &Path) -> Result<(String, String)> {
    use std::process::Command;
    
    let relative_path = path.strip_prefix(workspace_path)?;
    let relative_str = relative_path.to_str()
        .ok_or_else(|| anyhow!("Invalid path"))?;
    
    // Get blob ID
    let id_output = Command::new("git")
        .args(["rev-parse", &format!("HEAD:{}", relative_str)])
        .current_dir(workspace_path)
        .output()
        .context("Failed to get blob id")?;
    
    if !id_output.status.success() {
        anyhow::bail!("Failed to get blob id");
    }
    
    let id = String::from_utf8_lossy(&id_output.stdout).trim().to_string();
    
    // Get file content at HEAD
    let content_output = Command::new("git")
        .args(["show", &format!("HEAD:{}", relative_str)])
        .current_dir(workspace_path)
        .output()
        .context("Failed to get file content")?;
    
    if !content_output.status.success() {
        anyhow::bail!("Failed to get file content at HEAD");
    }
    
    let content = String::from_utf8(content_output.stdout)
        .context("content bytes to string")?;
    
    Ok((id, content))
}

fn git_get_remote_file_url(workspace_path: &Path, file: &Path) -> Result<String> {
    use std::process::Command;
    
    // Get current commit
    let commit_output = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(workspace_path)
        .output()
        .context("Failed to get HEAD commit")?;
    
    if !commit_output.status.success() {
        anyhow::bail!("Failed to get HEAD commit");
    }
    let commit = String::from_utf8_lossy(&commit_output.stdout).trim().to_string();
    
    // Get upstream remote name
    let remote_output = Command::new("git")
        .args(["rev-parse", "--abbrev-ref", "--symbolic-full-name", "@{u}"])
        .current_dir(workspace_path)
        .output()
        .context("Failed to get upstream")?;
    
    let remote_name = if remote_output.status.success() {
        let upstream = String::from_utf8_lossy(&remote_output.stdout).trim().to_string();
        upstream.split('/').next().unwrap_or("origin").to_string()
    } else {
        "origin".to_string()
    };
    
    // Get remote URL
    let url_output = Command::new("git")
        .args(["remote", "get-url", &remote_name])
        .current_dir(workspace_path)
        .output()
        .context("Failed to get remote URL")?;
    
    if !url_output.status.success() {
        anyhow::bail!("Failed to get remote URL");
    }
    
    let remote = String::from_utf8_lossy(&url_output.stdout).trim().to_string();
    
    let remote_url = match Url::parse(&remote) {
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
