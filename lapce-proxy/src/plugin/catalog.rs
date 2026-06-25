use std::{
    borrow::Cow,
    collections::HashMap,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    thread,
};

use lapce_rpc::{
    RpcError,
    dap_types::{self, DapId, DapServer, SetBreakpointsResponse},
    plugin::{PluginId, VoltID, VoltInfo, VoltMetadata},
    proxy::ProxyResponse,
    style::LineStyle,
};
use lapce_xi_rope::{Rope, RopeDelta};
use lsp_types::{
    DidOpenTextDocumentParams, MessageType, SemanticTokens, ShowMessageParams,
    TextDocumentIdentifier, TextDocumentItem, VersionedTextDocumentIdentifier,
    notification::DidOpenTextDocument, request::Request,
};
use parking_lot::Mutex;
use psp_types::Notification;
use serde_json::Value;

use super::{
    PluginCatalogNotification, PluginCatalogRpcHandler,
    dap::{DapClient, DapRpcHandler, DebuggerData},
    psp::{ClonableCallback, PluginServerRpc, PluginServerRpcHandler, RpcCallback},
    wasi::{load_all_volts, start_volt},
};
use crate::plugin::{
    install_volt, psp::PluginHandlerNotification, wasi::enable_volt,
};

pub struct PluginCatalog {
    workspace: Option<PathBuf>,
    plugin_rpc: PluginCatalogRpcHandler,
    plugins: HashMap<PluginId, PluginServerRpcHandler>,
    daps: HashMap<DapId, DapRpcHandler>,
    debuggers: HashMap<String, DebuggerData>,
    plugin_configurations: HashMap<String, HashMap<String, serde_json::Value>>,
    unactivated_volts: HashMap<VoltID, VoltMetadata>,
    open_files: HashMap<PathBuf, String>,
    native_lsps_started: Arc<Mutex<std::collections::HashSet<String>>>,
}

impl PluginCatalog {
    pub fn new(
        workspace: Option<PathBuf>,
        disabled_volts: Vec<VoltID>,
        extra_plugin_paths: Vec<PathBuf>,
        plugin_configurations: HashMap<String, HashMap<String, serde_json::Value>>,
        plugin_rpc: PluginCatalogRpcHandler,
    ) -> Self {
        let plugin = Self {
            workspace,
            plugin_rpc: plugin_rpc.clone(),
            plugin_configurations,
            plugins: HashMap::new(),
            daps: HashMap::new(),
            debuggers: HashMap::new(),
            unactivated_volts: HashMap::new(),
            open_files: HashMap::new(),
            native_lsps_started: Arc::new(Mutex::new(std::collections::HashSet::new())),
        };

        thread::spawn(move || {
            load_all_volts(plugin_rpc, &extra_plugin_paths, disabled_volts);
        });

        plugin
    }

    #[allow(clippy::too_many_arguments)]
    pub fn handle_server_request(
        &mut self,
        plugin_id: Option<PluginId>,
        request_sent: Option<Arc<AtomicUsize>>,
        method: Cow<'static, str>,
        params: Value,
        language_id: Option<String>,
        path: Option<PathBuf>,
        check: bool,
        f: Box<dyn ClonableCallback<Value, RpcError>>,
    ) {
        if let Some(plugin_id) = plugin_id {
            if let Some(plugin) = self.plugins.get(&plugin_id) {
                plugin.server_request_async(
                    method,
                    params,
                    language_id,
                    path,
                    check,
                    move |result| {
                        f(plugin_id, result);
                    },
                );
            } else {
                f(
                    plugin_id,
                    Err(RpcError {
                        code: 0,
                        message: "plugin doesn't exist".to_string(),
                    }),
                );
            }
            return;
        }

        if let Some(request_sent) = request_sent {
            // if there are no plugins installed the callback of the client is not called
            // so check if plugins list is empty
            if self.plugins.is_empty() {
                // Add a request
                request_sent.fetch_add(1, Ordering::Relaxed);

                // make a direct callback with an "error"
                f(
                    lapce_rpc::plugin::PluginId(0),
                    Err(RpcError {
                        code: 0,
                        message: "no available plugin could make a callback, because the plugins list is empty".to_string(),
                    }),
                );
                return;
            } else {
                request_sent.fetch_add(self.plugins.len(), Ordering::Relaxed);
            }
        }
        for (plugin_id, plugin) in self.plugins.iter() {
            let f = dyn_clone::clone_box(&*f);
            let plugin_id = *plugin_id;
            plugin.server_request_async(
                method.clone(),
                params.clone(),
                language_id.clone(),
                path.clone(),
                check,
                move |result| {
                    f(plugin_id, result);
                },
            );
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn handle_server_notification(
        &mut self,
        plugin_id: Option<PluginId>,
        method: impl Into<Cow<'static, str>>,
        params: Value,
        language_id: Option<String>,
        path: Option<PathBuf>,
        check: bool,
    ) {
        if let Some(plugin_id) = plugin_id {
            if let Some(plugin) = self.plugins.get(&plugin_id) {
                plugin.server_notification(method, params, language_id, path, check);
            }

            return;
        }

        // Otherwise send it to all plugins
        let method = method.into();
        for (_, plugin) in self.plugins.iter() {
            plugin.server_notification(
                method.clone(),
                params.clone(),
                language_id.clone(),
                path.clone(),
                check,
            );
        }
    }

    pub fn shutdown_volt(
        &mut self,
        volt: VoltInfo,
        f: Box<dyn ClonableCallback<Value, RpcError>>,
    ) {
        let id = volt.id();
        for (plugin_id, plugin) in self.plugins.iter() {
            if plugin.volt_id == id {
                let f = dyn_clone::clone_box(&*f);
                let plugin_id = *plugin_id;
                plugin.server_request_async(
                    lsp_types::request::Shutdown::METHOD,
                    Value::Null,
                    None,
                    None,
                    false,
                    move |result| {
                        f(plugin_id, result);
                    },
                );
                plugin.shutdown();
            }
        }
    }

    fn start_unactivated_volts(&mut self, to_be_activated: Vec<VoltID>) {
        for id in to_be_activated.iter() {
            let workspace = self.workspace.clone();
            if let Some(meta) = self.unactivated_volts.remove(id) {
                let configurations =
                    self.plugin_configurations.get(&meta.name).cloned();
                tracing::debug!("{:?} {:?}", id, configurations);
                let plugin_rpc = self.plugin_rpc.clone();
                thread::spawn(move || {
                    if let Err(err) =
                        start_volt(workspace, configurations, plugin_rpc, meta)
                    {
                        tracing::error!("{:?}", err);
                    }
                });
            }
        }
    }

    fn check_unactivated_volts(&mut self) {
        let to_be_activated: Vec<VoltID> = self
            .unactivated_volts
            .iter()
            .filter_map(|(id, meta)| {
                let contains = meta
                    .activation
                    .as_ref()
                    .and_then(|a| a.language.as_ref())
                    .map(|l| {
                        self.open_files
                            .iter()
                            .any(|(_, language_id)| l.contains(language_id))
                    })
                    .unwrap_or(false);
                if contains {
                    return Some(id.clone());
                }

                if let Some(workspace) = self.workspace.as_ref() {
                    if let Some(globs) = meta
                        .activation
                        .as_ref()
                        .and_then(|a| a.workspace_contains.as_ref())
                    {
                        let mut builder = globset::GlobSetBuilder::new();
                        for glob in globs {
                            match globset::Glob::new(glob) {
                                Ok(glob) => {
                                    builder.add(glob);
                                }
                                Err(err) => {
                                    tracing::error!("{:?}", err);
                                }
                            }
                        }
                        match builder.build() {
                            Ok(matcher) => {
                                if !matcher.is_empty() {
                                    for entry in walkdir::WalkDir::new(workspace)
                                        .into_iter()
                                        .flatten()
                                    {
                                        if matcher.is_match(entry.path()) {
                                            return Some(id.clone());
                                        }
                                    }
                                }
                            }
                            Err(err) => {
                                tracing::error!("{:?}", err);
                            }
                        }
                    }
                }

                None
            })
            .collect();
        self.start_unactivated_volts(to_be_activated);
    }

    pub fn handle_did_open_text_document(&mut self, document: TextDocumentItem) {
        match document.uri.to_file_path() {
            Ok(path) => {
                self.open_files.insert(path, document.language_id.clone());
            }
            Err(err) => {
                tracing::error!("{:?}", err);
            }
        }

        let to_be_activated: Vec<VoltID> = self
            .unactivated_volts
            .iter()
            .filter_map(|(id, meta)| {
                let contains = meta
                    .activation
                    .as_ref()
                    .and_then(|a| a.language.as_ref())
                    .map(|l| l.contains(&document.language_id))?;
                if contains { Some(id.clone()) } else { None }
            })
            .collect();
        self.start_unactivated_volts(to_be_activated);

        // Native LSP auto-install logic
        {
            let mut started = self.native_lsps_started.lock();
            if !started.contains(&document.language_id) {
                started.insert(document.language_id.clone());
                drop(started);
                self.start_native_lsp_for_language(&document.language_id);
            }
        }

        let path = document.uri.to_file_path().ok();
        if document.language_id == "python" {
            if let Some(ref file_path) = path {
                let mut current_dir = file_path.parent();
                let mut python_path = None;
                while let Some(dir) = current_dir {
                    for venv_name in &[".venv", "venv", "env", ".conda", "conda"] {
                        let venv_path = dir.join(venv_name);
                        #[cfg(target_os = "windows")]
                        let bin_path = venv_path.join("Scripts").join("python.exe");
                        #[cfg(not(target_os = "windows"))]
                        let bin_path = venv_path.join("bin").join("python");

                        if bin_path.exists() {
                            python_path = Some(bin_path);
                            break;
                        }
                    }
                    if python_path.is_some() {
                        break;
                    }
                    if Some(dir) == self.workspace.as_deref() {
                        break;
                    }
                    current_dir = dir.parent();
                }

                if let Some(python_path) = python_path {
                    tracing::info!("Found python virtual env for {:?}: {:?}", file_path, python_path);
                    for (_, plugin) in self.plugins.iter() {
                        if plugin.volt_id.name == "pyright" {
                            let settings = serde_json::json!({
                                "python": {
                                    "pythonPath": python_path.to_string_lossy()
                                }
                            });
                            tracing::info!("Sending pythonPath to pyright for {:?}: {:?}", file_path, python_path);
                            plugin.server_notification(
                                lsp_types::notification::DidChangeConfiguration::METHOD,
                                lsp_types::DidChangeConfigurationParams { settings },
                                Some(document.language_id.clone()),
                                Some(file_path.clone()),
                                false,
                            );
                        }
                    }
                }
            }
        }

        for (_, plugin) in self.plugins.iter() {
            plugin.server_notification(
                DidOpenTextDocument::METHOD,
                DidOpenTextDocumentParams {
                    text_document: document.clone(),
                },
                Some(document.language_id.clone()),
                path.clone(),
                true,
            );
        }
    }

    fn start_native_lsp_for_language(&self, language_id: &str) {
        let (tool_name, lsp_binary, mut args) = match language_id {
            "python" => ("pyright", "pyright-langserver", vec!["--stdio".to_string()]),
            "rust" => ("rust-analyzer", "rust-analyzer", vec![]),
            "go" => ("gopls", "gopls", vec![]),
            "javascript" | "typescript" | "javascriptreact" | "typescriptreact" => {
                ("typescript-language-server", "typescript-language-server", vec!["--stdio".to_string()])
            }
            "svelte" | "vue" | "astro" => ("typescript-language-server", "typescript-language-server", vec!["--stdio".to_string()]),
            _ => return, // Unsupported language for native auto-install
        };

        let catalog_rpc = self.plugin_rpc.clone();
        let workspace = self.workspace.clone();
        let language_id = language_id.to_string();
        let native_lsps_started = self.native_lsps_started.clone();

        std::thread::spawn(move || {
            let core_rpc = catalog_rpc.core_rpc.clone();
            core_rpc.log(
                lapce_rpc::core::LogLevel::Info,
                format!("Checking native LSP for {}: {}", language_id, tool_name),
                None,
            );

            // 1. Prefer bundled binary, then PATH, then proto install
            let mut bin_path = crate::bundled_lsp::resolve_lsp_binary(lsp_binary)
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_else(|| lsp_binary.to_string());

            if let Some(bundled) = crate::bundled_lsp::bundled_lsp_binary(lsp_binary) {
                core_rpc.log(
                    lapce_rpc::core::LogLevel::Info,
                    format!(
                        "Using bundled LSP for {}: {}",
                        language_id,
                        bundled.display()
                    ),
                    None,
                );
            } else if !std::path::Path::new(&bin_path).exists() {
                let proto_bin = crate::proto_manager::proto_bin();
                let proto_available = std::process::Command::new(&proto_bin)
                    .arg("--version")
                    .output()
                    .is_ok();

                if proto_available {
                    core_rpc.log(
                        lapce_rpc::core::LogLevel::Info,
                        format!("Installing {} via proto...", tool_name),
                        None,
                    );

                    let _ = std::process::Command::new(&proto_bin)
                        .arg("install")
                        .arg(tool_name)
                        .output();

                    if let Ok(output) = std::process::Command::new(&proto_bin)
                        .arg("bin")
                        .arg(tool_name)
                        .output()
                    {
                        if let Ok(path) = String::from_utf8(output.stdout) {
                            let path = path.trim();
                            if !path.is_empty() {
                                bin_path = path.to_string();
                            }
                        }
                    }
                }

                if !std::path::Path::new(&bin_path).exists() {
                    if let Some(found) = crate::bundled_lsp::path_lookup(lsp_binary) {
                        bin_path = found.to_string_lossy().into_owned();
                    }
                }
            }

            core_rpc.log(
                lapce_rpc::core::LogLevel::Info,
                format!("Starting native LSP for {}: {}", language_id, bin_path),
                None,
            );

            // 2. Prepare the LSP start parameters
            let volt_id = VoltID {
                author: "forge".to_string(),
                name: tool_name.to_string(),
            };

            let server_uri = if std::path::Path::new(&bin_path).is_absolute() {
                lsp_types::Url::from_file_path(&bin_path).unwrap_or_else(|_| {
                    lsp_types::Url::parse(&format!("urn:{}", bin_path)).unwrap()
                })
            } else {
                lsp_types::Url::parse(&format!("urn:{}", bin_path)).unwrap_or_else(|_| {
                    lsp_types::Url::parse(&format!("urn:{}", lsp_binary)).unwrap()
                })
            };
            
            let document_selector = vec![lsp_types::DocumentFilter {
                language: Some(language_id.clone()),
                scheme: Some("file".to_string()),
                pattern: None,
            }];

            // 3. Start the LSP
            if let Err(err) = crate::plugin::lsp::LspClient::start(
                catalog_rpc.clone(),
                document_selector,
                workspace.clone(),
                volt_id,
                format!("{} (Native)", tool_name),
                None,
                None,
                workspace,
                server_uri,
                args,
                None,
            ) {
                native_lsps_started.lock().remove(&language_id);
                tracing::error!("Failed to start native LSP: {:?}", err);
                core_rpc.log(
                    lapce_rpc::core::LogLevel::Error,
                    format!("Failed to start native LSP for {}: {:?}", language_id, err),
                    None,
                );
            }
        });
    }

    pub fn handle_did_save_text_document(
        &mut self,
        language_id: String,
        path: PathBuf,
        text_document: TextDocumentIdentifier,
        text: Rope,
    ) {
        for (_, plugin) in self.plugins.iter() {
            plugin.handle_rpc(PluginServerRpc::DidSaveTextDocument {
                language_id: language_id.clone(),
                path: path.clone(),
                text_document: text_document.clone(),
                text: text.clone(),
            });
        }
    }

    pub fn handle_did_change_text_document(
        &mut self,
        language_id: String,
        document: VersionedTextDocumentIdentifier,
        delta: RopeDelta,
        text: Rope,
        new_text: Rope,
    ) {
        let change = Arc::new(Mutex::new((None, None)));
        for (_, plugin) in self.plugins.iter() {
            plugin.handle_rpc(PluginServerRpc::DidChangeTextDocument {
                language_id: language_id.clone(),
                document: document.clone(),
                delta: delta.clone(),
                text: text.clone(),
                new_text: new_text.clone(),
                change: change.clone(),
            });
        }
    }

    pub fn format_semantic_tokens(
        &self,
        plugin_id: PluginId,
        tokens: SemanticTokens,
        text: Rope,
        f: Box<dyn RpcCallback<Vec<LineStyle>, RpcError>>,
    ) {
        if let Some(plugin) = self.plugins.get(&plugin_id) {
            plugin.handle_rpc(PluginServerRpc::FormatSemanticTokens {
                tokens,
                text,
                f,
            });
        } else {
            f.call(Err(RpcError {
                code: 0,
                message: "plugin doesn't exist".to_string(),
            }));
        }
    }

    pub fn dap_variable(
        &self,
        dap_id: DapId,
        reference: usize,
        f: Box<dyn RpcCallback<Vec<dap_types::Variable>, RpcError>>,
    ) {
        if let Some(dap) = self.daps.get(&dap_id) {
            dap.variables_async(
                reference,
                |result: Result<dap_types::VariablesResponse, RpcError>| {
                    f.call(result.map(|resp| resp.variables))
                },
            );
        } else {
            f.call(Err(RpcError {
                code: 0,
                message: "plugin doesn't exist".to_string(),
            }));
        }
    }

    pub fn dap_get_scopes(
        &self,
        dap_id: DapId,
        frame_id: usize,
        f: Box<
            dyn RpcCallback<
                    Vec<(dap_types::Scope, Vec<dap_types::Variable>)>,
                    RpcError,
                >,
        >,
    ) {
        if let Some(dap) = self.daps.get(&dap_id) {
            let local_dap = dap.clone();
            dap.scopes_async(
                frame_id,
                move |result: Result<dap_types::ScopesResponse, RpcError>| {
                    match result {
                        Ok(resp) => {
                            let scopes = resp.scopes.clone();
                            if let Some(scope) = resp.scopes.first() {
                                let scope = scope.to_owned();
                                thread::spawn(move || {
                                    local_dap.variables_async(
                                        scope.variables_reference,
                                        move |result: Result<
                                            dap_types::VariablesResponse,
                                            RpcError,
                                        >| {
                                            let resp: Vec<(
                                                dap_types::Scope,
                                                Vec<dap_types::Variable>,
                                            )> = scopes
                                                .iter()
                                                .enumerate()
                                                .map(|(index, s)| {
                                                    (
                                                        s.clone(),
                                                        if index == 0 {
                                                            result
                                                                .as_ref()
                                                                .map(|resp| {
                                                                    resp.variables
                                                                        .clone()
                                                                })
                                                                .unwrap_or_default()
                                                        } else {
                                                            Vec::new()
                                                        },
                                                    )
                                                })
                                                .collect();
                                            f.call(Ok(resp));
                                        },
                                    );
                                });
                            } else {
                                f.call(Ok(Vec::new()));
                            }
                        }
                        Err(e) => {
                            f.call(Err(e));
                        }
                    }
                },
            );
        } else {
            f.call(Err(RpcError {
                code: 0,
                message: "plugin doesn't exist".to_string(),
            }));
        }
    }

    pub fn handle_notification(&mut self, notification: PluginCatalogNotification) {
        use PluginCatalogNotification::*;
        match notification {
            UnactivatedVolts(volts) => {
                tracing::debug!("UnactivatedVolts {:?}", volts);
                for volt in volts {
                    let id = volt.id();
                    self.unactivated_volts.insert(id, volt);
                }
                self.check_unactivated_volts();
            }
            UpdatePluginConfigs(configs) => {
                tracing::debug!("UpdatePluginConfigs {:?}", configs);
                self.plugin_configurations = configs;
            }
            PluginServerLoaded(plugin) => {
                // TODO: check if the server has did open registered
                match self.plugin_rpc.proxy_rpc.get_open_files_content() {
                    Ok(ProxyResponse::GetOpenFilesContentResponse { items }) => {
                        for item in items {
                            let language_id = Some(item.language_id.clone());
                            let path = item.uri.to_file_path().ok();
                            plugin.server_notification(
                                DidOpenTextDocument::METHOD,
                                DidOpenTextDocumentParams {
                                    text_document: item,
                                },
                                language_id,
                                path,
                                true,
                            );
                        }
                    }
                    Ok(_) => {}
                    Err(err) => {
                        tracing::error!("{:?}", err);
                    }
                }

                let plugin_id = plugin.plugin_id;
                let spawned_by = plugin.spawned_by;

                self.plugins.insert(plugin.plugin_id, plugin);

                if let Some(spawned_by) = spawned_by {
                    if let Some(plugin) = self.plugins.get(&spawned_by) {
                        plugin.handle_rpc(PluginServerRpc::Handler(
                            PluginHandlerNotification::SpawnedPluginLoaded {
                                plugin_id,
                            },
                        ));
                    }
                }
            }
            NativeLspStopped {
                language_id,
                plugin_id,
            } => {
                self.native_lsps_started.lock().remove(&language_id);
                self.plugins.remove(&plugin_id);
            }
            InstallVolt(volt) => {
                tracing::debug!("InstallVolt {:?}", volt);
                let workspace = self.workspace.clone();
                let configurations =
                    self.plugin_configurations.get(&volt.name).cloned();
                let catalog_rpc = self.plugin_rpc.clone();
                catalog_rpc.stop_volt(volt.clone());
                thread::spawn(move || {
                    if let Err(err) =
                        install_volt(catalog_rpc, workspace, configurations, volt)
                    {
                        tracing::error!("{:?}", err);
                    }
                });
            }
            ReloadVolt(volt) => {
                tracing::debug!("ReloadVolt {:?}", volt);
                let volt_id = volt.id();
                let ids: Vec<PluginId> = self.plugins.keys().cloned().collect();
                for id in ids {
                    if self.plugins.get(&id).unwrap().volt_id == volt_id {
                        let plugin = self.plugins.remove(&id).unwrap();
                        plugin.shutdown();
                    }
                }
                if let Err(err) = self.plugin_rpc.unactivated_volts(vec![volt]) {
                    tracing::error!("{:?}", err);
                }
            }
            StopVolt(volt) => {
                tracing::debug!("StopVolt {:?}", volt);
                let volt_id = volt.id();
                let ids: Vec<PluginId> = self.plugins.keys().cloned().collect();
                for id in ids {
                    if self.plugins.get(&id).unwrap().volt_id == volt_id {
                        let plugin = self.plugins.remove(&id).unwrap();
                        plugin.shutdown();
                    }
                }
            }
            EnableVolt(volt) => {
                tracing::debug!("EnableVolt {:?}", volt);
                let volt_id = volt.id();
                for (_, volt) in self.plugins.iter() {
                    if volt.volt_id == volt_id {
                        return;
                    }
                }
                let plugin_rpc = self.plugin_rpc.clone();
                thread::spawn(move || {
                    if let Err(err) = enable_volt(plugin_rpc, volt) {
                        tracing::error!("{:?}", err);
                    }
                });
            }
            DapLoaded(dap_rpc) => {
                self.daps.insert(dap_rpc.dap_id, dap_rpc);
            }
            DapDisconnected(dap_id) => {
                self.daps.remove(&dap_id);
            }
            DapStart {
                config,
                breakpoints,
            } => {
                let workspace = self.workspace.clone();
                let plugin_rpc = self.plugin_rpc.clone();
                if let Some(debugger) = config
                    .ty
                    .as_ref()
                    .and_then(|ty| self.debuggers.get(ty).cloned())
                {
                    thread::spawn(move || {
                        match DapClient::start(
                            DapServer {
                                program: debugger.program,
                                args: debugger.args.unwrap_or_default(),
                                cwd: workspace,
                            },
                            config.clone(),
                            breakpoints,
                            plugin_rpc.clone(),
                        ) {
                            Ok(dap_rpc) => {
                                if let Err(err) =
                                    plugin_rpc.dap_loaded(dap_rpc.clone())
                                {
                                    tracing::error!("{:?}", err);
                                }

                                if let Err(err) = dap_rpc.launch(&config) {
                                    tracing::error!("{:?}", err);
                                }
                            }
                            Err(err) => {
                                tracing::error!("{:?}", err);
                            }
                        }
                    });
                } else {
                    self.plugin_rpc.core_rpc.show_message(
                        "debug fail".to_owned(),
                        ShowMessageParams {
                            typ: MessageType::ERROR,
                            message: "Debugger not found. Please install the appropriate plugin.".to_owned(),
                        },
                    )
                }
            }
            DapProcessId {
                dap_id,
                process_id,
                term_id,
            } => {
                if let Some(dap) = self.daps.get(&dap_id) {
                    if let Err(err) =
                        dap.termain_process_tx.send((term_id, process_id))
                    {
                        tracing::error!("{:?}", err);
                    }
                }
            }
            DapContinue { dap_id, thread_id } => {
                if let Some(dap) = self.daps.get(&dap_id).cloned() {
                    let plugin_rpc = self.plugin_rpc.clone();
                    thread::spawn(move || {
                        if dap.continue_thread(thread_id).is_ok() {
                            plugin_rpc.core_rpc.dap_continued(dap_id);
                        }
                    });
                }
            }
            DapPause { dap_id, thread_id } => {
                if let Some(dap) = self.daps.get(&dap_id).cloned() {
                    thread::spawn(move || {
                        if let Err(err) = dap.pause_thread(thread_id) {
                            tracing::error!("{:?}", err);
                        }
                    });
                }
            }
            DapStepOver { dap_id, thread_id } => {
                if let Some(dap) = self.daps.get(&dap_id).cloned() {
                    dap.next(thread_id);
                }
            }
            DapStepInto { dap_id, thread_id } => {
                if let Some(dap) = self.daps.get(&dap_id).cloned() {
                    dap.step_in(thread_id);
                }
            }
            DapStepOut { dap_id, thread_id } => {
                if let Some(dap) = self.daps.get(&dap_id).cloned() {
                    dap.step_out(thread_id);
                }
            }
            DapStop { dap_id } => {
                if let Some(dap) = self.daps.get(&dap_id) {
                    dap.stop();
                }
            }
            DapDisconnect { dap_id } => {
                if let Some(dap) = self.daps.get(&dap_id).cloned() {
                    thread::spawn(move || {
                        if let Err(err) = dap.disconnect() {
                            tracing::error!("{:?}", err);
                        }
                    });
                }
            }
            DapRestart {
                dap_id,
                breakpoints,
            } => {
                if let Some(dap) = self.daps.get(&dap_id) {
                    dap.restart(breakpoints);
                }
            }
            DapSetBreakpoints {
                dap_id,
                path,
                breakpoints,
            } => {
                if let Some(dap) = self.daps.get(&dap_id) {
                    let core_rpc = self.plugin_rpc.core_rpc.clone();
                    dap.set_breakpoints_async(
                        path.clone(),
                        breakpoints,
                        move |result: Result<SetBreakpointsResponse, RpcError>| {
                            match result {
                                Ok(resp) => {
                                    core_rpc.dap_breakpoints_resp(
                                        dap_id,
                                        path,
                                        resp.breakpoints.unwrap_or_default(),
                                    );
                                }
                                Err(err) => {
                                    tracing::error!("{:?}", err);
                                }
                            }
                        },
                    );
                }
            }
            RegisterDebuggerType {
                debugger_type,
                program,
                args,
            } => {
                self.debuggers.insert(
                    debugger_type.clone(),
                    DebuggerData {
                        debugger_type,
                        program,
                        args,
                    },
                );
            }
            Shutdown => {
                for (_, plugin) in self.plugins.iter() {
                    plugin.shutdown();
                }
            }
        }
    }
}
