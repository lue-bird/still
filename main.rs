// lsp still reports this specific error even when it is allowed in the cargo.toml
#![allow(non_upper_case_globals)]

struct State {
    projects: std::collections::HashMap<std::path::PathBuf, ProjectState>,
    open_still_text_document_uris: std::collections::HashSet<lsp_types::Url>,
}

struct ProjectState {
    syntax: StillSyntaxProject,
    source: String,
    problems: Vec<FileInternalCompileProblem>,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let (connection, io_thread) = lsp_server::Connection::stdio();

    let (initialize_request_id, initialize_arguments_json) = connection.initialize_start()?;
    connection.initialize_finish(
        initialize_request_id,
        serde_json::to_value(lsp_types::InitializeResult {
            capabilities: server_capabilities(),
            server_info: Some(lsp_types::ServerInfo {
                name: "still".to_string(),
                version: Some("0.0.1".to_string()),
            }),
        })?,
    )?;
    let initialize_arguments: lsp_types::InitializeParams =
        serde_json::from_value(initialize_arguments_json)?;
    let state: State = initialize(&connection, &initialize_arguments)?;
    server_loop(&connection, state)?;
    // shut down gracefully
    drop(connection);
    io_thread.join()?;
    Ok(())
}
fn initialize(
    connection: &lsp_server::Connection,
    initialize_arguments: &lsp_types::InitializeParams,
) -> Result<State, Box<dyn std::error::Error>> {
    let state: State = State {
        projects: initialize_projects_state_for_workspace_directories_into(
            connection,
            initialize_arguments,
        ),
        open_still_text_document_uris: std::collections::HashSet::new(),
    };
    connection.sender.send(lsp_server::Message::Notification(
        lsp_server::Notification {
            method: <lsp_types::request::RegisterCapability as lsp_types::request::Request>::METHOD
                .to_string(),
            params: serde_json::to_value(lsp_types::RegistrationParams {
                registrations: initial_additional_capability_registrations(&state)?,
            })?,
        },
    ))?;
    Ok(state)
}
fn initial_additional_capability_registrations(
    state: &State,
) -> Result<Vec<lsp_types::Registration>, Box<dyn std::error::Error>> {
    let file_watch_registration_options: lsp_types::DidChangeWatchedFilesRegistrationOptions =
        lsp_types::DidChangeWatchedFilesRegistrationOptions {
            watchers: state
                .projects
                .keys()
                .filter_map(|source_directory_path| {
                    lsp_types::Url::from_directory_path(source_directory_path).ok()
                })
                .map(|source_directory_url| lsp_types::FileSystemWatcher {
                    glob_pattern: lsp_types::GlobPattern::Relative(lsp_types::RelativePattern {
                        base_uri: lsp_types::OneOf::Right(source_directory_url),
                        pattern: "**/*.still".to_string(),
                    }),
                    kind: Some(
                        lsp_types::WatchKind::Create
                            | lsp_types::WatchKind::Change
                            | lsp_types::WatchKind::Delete,
                    ),
                })
                .collect::<Vec<lsp_types::FileSystemWatcher>>(),
        };
    let file_watch_registration_options_json: serde_json::Value =
        serde_json::to_value(file_watch_registration_options)?;
    let file_watch_registration: lsp_types::Registration = lsp_types::Registration {
        id: "file-watch".to_string(),
        method: <lsp_types::notification::DidChangeWatchedFiles as lsp_types::notification::Notification>::METHOD.to_string(),
        register_options: Some(file_watch_registration_options_json),
    };
    Ok(vec![file_watch_registration])
}
fn server_capabilities() -> lsp_types::ServerCapabilities {
    lsp_types::ServerCapabilities {
        hover_provider: Some(lsp_types::HoverProviderCapability::Simple(true)),
        definition_provider: Some(lsp_types::OneOf::Left(true)),
        semantic_tokens_provider: Some(
            lsp_types::SemanticTokensServerCapabilities::SemanticTokensOptions(
                lsp_types::SemanticTokensOptions {
                    work_done_progress_options: lsp_types::WorkDoneProgressOptions {
                        work_done_progress: None,
                    },
                    legend: lsp_types::SemanticTokensLegend {
                        token_modifiers: vec![],
                        token_types: Vec::from(token_types),
                    },
                    range: None,
                    full: Some(lsp_types::SemanticTokensFullOptions::Bool(true)),
                },
            ),
        ),
        text_document_sync: Some(lsp_types::TextDocumentSyncCapability::Kind(
            lsp_types::TextDocumentSyncKind::INCREMENTAL,
        )),
        rename_provider: Some(lsp_types::OneOf::Right(lsp_types::RenameOptions {
            prepare_provider: Some(true),
            work_done_progress_options: lsp_types::WorkDoneProgressOptions {
                work_done_progress: None,
            },
        })),
        references_provider: Some(lsp_types::OneOf::Left(true)),
        completion_provider: Some(lsp_types::CompletionOptions {
            resolve_provider: Some(false),
            trigger_characters: Some(vec![".".to_string()]),
            all_commit_characters: None,
            work_done_progress_options: lsp_types::WorkDoneProgressOptions {
                work_done_progress: None,
            },
            completion_item: Some(lsp_types::CompletionOptionsCompletionItem {
                label_details_support: None,
            }),
        }),
        document_formatting_provider: Some(lsp_types::OneOf::Left(true)),
        document_symbol_provider: Some(lsp_types::OneOf::Left(true)),
        ..lsp_types::ServerCapabilities::default()
    }
}
fn server_loop(
    connection: &lsp_server::Connection,
    mut state: State,
) -> Result<(), Box<dyn std::error::Error>> {
    for client_message in &connection.receiver {
        match client_message {
            lsp_server::Message::Request(request) => {
                if connection.handle_shutdown(&request)? {
                    break;
                }
                if let Err(error) = handle_request(
                    connection,
                    &state,
                    request.id,
                    &request.method,
                    request.params,
                ) {
                    eprintln!("request {} failed: {error}", &request.method);
                }
            }
            lsp_server::Message::Notification(notification) => {
                if let Err(err) = handle_notification(
                    connection,
                    &mut state,
                    &notification.method,
                    notification.params,
                ) {
                    eprintln!("notification {} failed: {err}", notification.method);
                }
            }
            lsp_server::Message::Response(_) => {}
        }
    }
    Ok(())
}
fn handle_notification(
    connection: &lsp_server::Connection,
    state: &mut State,
    notification_method: &str,
    notification_arguments_json: serde_json::Value,
) -> Result<(), Box<dyn std::error::Error>> {
    match notification_method {
        <lsp_types::notification::DidOpenTextDocument as lsp_types::notification::Notification>::METHOD => {
            let arguments: <lsp_types::notification::DidOpenTextDocument as lsp_types::notification::Notification>::Params =
                serde_json::from_value(notification_arguments_json)?;
            update_state_on_did_open_text_document(state, connection, arguments);
        }
        <lsp_types::notification::DidCloseTextDocument as lsp_types::notification::Notification>::METHOD => {
            let arguments: <lsp_types::notification::DidCloseTextDocument as lsp_types::notification::Notification>::Params =
                serde_json::from_value(notification_arguments_json)?;
            state.open_still_text_document_uris.remove(&arguments.text_document.uri);
        }
        <lsp_types::notification::DidChangeTextDocument as lsp_types::notification::Notification>::METHOD => {
            let arguments: <lsp_types::notification::DidChangeTextDocument as lsp_types::notification::Notification>::Params =
                serde_json::from_value(notification_arguments_json)?;
            update_state_on_did_change_text_document(state, connection, arguments);
        }
        <lsp_types::notification::DidChangeWatchedFiles as lsp_types::notification::Notification>::METHOD => {
            let arguments: <lsp_types::notification::DidChangeWatchedFiles as lsp_types::notification::Notification>::Params =
                serde_json::from_value(notification_arguments_json)?;
            update_state_on_did_change_watched_files(connection, state, arguments);
        }
        <lsp_types::notification::Exit as lsp_types::notification::Notification>::METHOD => {}
        _ => {}
    }
    Ok(())
}
fn update_state_on_did_open_text_document(
    state: &mut State,
    connection: &lsp_server::Connection,
    arguments: lsp_types::DidOpenTextDocumentParams,
) {
    // Why is the existing handling on DidChangeWatchedFiles not good enough?
    // When moving a project into an existing project,
    // no syntax highlighting would be shown before you interact with the file,
    // as semantic tokens are requested before the DidChangeWatchedFiles notification is sent.
    // Since DidOpenTextDocumentParams already sends the full file content anyway,
    // handling it on document open is relatively cheap and straightforward
    if let Ok(opened_path) = arguments.text_document.uri.to_file_path()
        && opened_path.extension().is_some_and(|ext| ext == "still")
    {
        state.projects.entry(opened_path).or_insert_with(|| {
            initialize_project_state_from_source(
                connection,
                arguments.text_document.uri.clone(),
                arguments.text_document.text,
            )
        });
        state
            .open_still_text_document_uris
            .insert(arguments.text_document.uri);
    }
}
fn update_state_on_did_change_watched_files(
    connection: &lsp_server::Connection,
    state: &mut State,
    arguments: lsp_types::DidChangeWatchedFilesParams,
) {
    for (changed_file_path, file_change) in
        arguments
            .changes
            .into_iter()
            .filter_map(|file_change_event| {
                if file_change_event.typ == lsp_types::FileChangeType::CHANGED
                    && state
                        .open_still_text_document_uris
                        .contains(&file_change_event.uri)
                {
                    None
                } else {
                    match file_change_event.uri.to_file_path() {
                        Ok(changed_file_path) => Some((changed_file_path, file_change_event)),
                        Err(()) => None,
                    }
                }
            })
    {
        match file_change.typ {
            lsp_types::FileChangeType::DELETED => {
                if state.projects.remove(&changed_file_path).is_some() {
                    publish_diagnostics(
                        connection,
                        lsp_types::PublishDiagnosticsParams {
                            uri: file_change.uri,
                            diagnostics: vec![],
                            version: None,
                        },
                    );
                }
            }
            lsp_types::FileChangeType::CREATED | lsp_types::FileChangeType::CHANGED => {
                if changed_file_path
                    .extension()
                    .is_some_and(|ext| ext == "still")
                {
                    match std::fs::read_to_string(&changed_file_path) {
                        Err(_) => {}
                        Ok(changed_file_source) => {
                            let changed_project_state = initialize_project_state_from_source(
                                connection,
                                file_change.uri,
                                changed_file_source,
                            );
                            state
                                .projects
                                .insert(changed_file_path, changed_project_state);
                        }
                    }
                }
            }
            unknown_file_change_type => {
                eprintln!(
                    "unknown file change type sent by LSP client: {:?}",
                    unknown_file_change_type
                );
            }
        }
    }
}

fn handle_request(
    connection: &lsp_server::Connection,
    state: &State,
    request_id: lsp_server::RequestId,
    request_method: &str,
    request_arguments_json: serde_json::Value,
) -> Result<(), Box<dyn std::error::Error>> {
    let response: Result<serde_json::Value, lsp_server::ResponseError> = match request_method {
        <lsp_types::request::HoverRequest as lsp_types::request::Request>::METHOD => {
            let arguments: <lsp_types::request::HoverRequest as lsp_types::request::Request>::Params =
                serde_json::from_value(request_arguments_json)?;
            let maybe_hover_result: <lsp_types::request::HoverRequest as lsp_types::request::Request>::Result =
                respond_to_hover(state, &arguments);
            Ok(serde_json::to_value(maybe_hover_result)?)
        }
        <lsp_types::request::GotoDefinition as lsp_types::request::Request>::METHOD => {
            let arguments: <lsp_types::request::GotoDefinition as lsp_types::request::Request>::Params =
                serde_json::from_value(request_arguments_json)?;
            let maybe_hover_result: <lsp_types::request::GotoDefinition as lsp_types::request::Request>::Result =
                respond_to_goto_definition(state, arguments);
            Ok(serde_json::to_value(maybe_hover_result)?)
        }
        <lsp_types::request::PrepareRenameRequest as lsp_types::request::Request>::METHOD => {
            let prepare_rename_arguments: <lsp_types::request::PrepareRenameRequest as lsp_types::request::Request>::Params =
                serde_json::from_value(request_arguments_json)?;
            let prepared: Option<
                Result<lsp_types::PrepareRenameResponse, lsp_server::ResponseError>,
            > = respond_to_prepare_rename(state, &prepare_rename_arguments);
            let response_result: Result<
                <lsp_types::request::PrepareRenameRequest as lsp_types::request::Request>::Result,
                lsp_server::ResponseError,
            > = match prepared {
                None => Ok(None),
                Some(result) => result.map(Some),
            };
            match response_result {
                Err(error) => Err(error),
                Ok(maybe_response) => Ok(serde_json::to_value(maybe_response)?),
            }
        }
        <lsp_types::request::Rename as lsp_types::request::Request>::METHOD => {
            let arguments: <lsp_types::request::Rename as lsp_types::request::Request>::Params =
                serde_json::from_value(request_arguments_json)?;
            let maybe_rename_edits: Option<Vec<lsp_types::TextDocumentEdit>> =
                respond_to_rename(state, arguments);
            let result: <lsp_types::request::Rename as lsp_types::request::Request>::Result =
                maybe_rename_edits.map(|rename_edits| lsp_types::WorkspaceEdit {
                    changes: None,
                    document_changes: Some(lsp_types::DocumentChanges::Edits(rename_edits)),
                    change_annotations: None,
                });
            Ok(serde_json::to_value(result)?)
        }
        <lsp_types::request::References as lsp_types::request::Request>::METHOD => {
            let arguments: <lsp_types::request::References as lsp_types::request::Request>::Params =
                serde_json::from_value(request_arguments_json)?;
            let result: <lsp_types::request::References as lsp_types::request::Request>::Result =
                respond_to_references(state, arguments);
            Ok(serde_json::to_value(result)?)
        }
        <lsp_types::request::SemanticTokensFullRequest as lsp_types::request::Request>::METHOD => {
            let arguments: <lsp_types::request::SemanticTokensFullRequest as lsp_types::request::Request>::Params =
                serde_json::from_value(request_arguments_json)?;
            let result: <lsp_types::request::SemanticTokensFullRequest as lsp_types::request::Request>::Result =
                respond_to_semantic_tokens_full(state, &arguments);
            Ok(serde_json::to_value(result)?)
        }
        <lsp_types::request::Completion as lsp_types::request::Request>::METHOD => {
            let arguments: <lsp_types::request::Completion as lsp_types::request::Request>::Params =
                serde_json::from_value(request_arguments_json)?;
            let result: <lsp_types::request::Completion as lsp_types::request::Request>::Result =
                respond_to_completion(state, &arguments);
            Ok(serde_json::to_value(result)?)
        }
        <lsp_types::request::Formatting as lsp_types::request::Request>::METHOD => {
            let arguments: <lsp_types::request::Formatting as lsp_types::request::Request>::Params =
                serde_json::from_value(request_arguments_json)?;
            let result: <lsp_types::request::Formatting as lsp_types::request::Request>::Result =
                respond_to_document_formatting(state, &arguments);
            Ok(serde_json::to_value(result)?)
        }
        <lsp_types::request::DocumentSymbolRequest as lsp_types::request::Request>::METHOD => {
            let arguments: <lsp_types::request::DocumentSymbolRequest as lsp_types::request::Request>::Params =
                serde_json::from_value(request_arguments_json)?;
            let result: <lsp_types::request::DocumentSymbolRequest as lsp_types::request::Request>::Result =
                respond_to_document_symbols(state, &arguments);
            Ok(serde_json::to_value(result)?)
        }
        <lsp_types::request::Shutdown as lsp_types::request::Request>::METHOD => {
            let result: <lsp_types::request::Shutdown as lsp_types::request::Request>::Result = ();
            Ok(serde_json::to_value(result)?)
        }
        _ => Err(lsp_server::ResponseError {
            code: lsp_server::ErrorCode::MethodNotFound as i32,
            message: "unhandled method".to_string(),
            data: None,
        }),
    };
    match response {
        Ok(response_value) => {
            send_response_ok(connection, request_id, response_value)?;
        }
        Err(response_error) => send_response_error(connection, request_id, response_error)?,
    }
    Ok(())
}

fn send_response_ok(
    connection: &lsp_server::Connection,
    id: lsp_server::RequestId,
    result: serde_json::Value,
) -> Result<(), Box<dyn std::error::Error>> {
    let response: lsp_server::Response = lsp_server::Response {
        id,
        result: Some(result),
        error: None,
    };
    connection
        .sender
        .send(lsp_server::Message::Response(response))?;
    Ok(())
}
fn send_response_error(
    connection: &lsp_server::Connection,
    id: lsp_server::RequestId,
    error: lsp_server::ResponseError,
) -> Result<(), Box<dyn std::error::Error>> {
    let response: lsp_server::Response = lsp_server::Response {
        id,
        result: None,
        error: Some(error),
    };
    connection
        .sender
        .send(lsp_server::Message::Response(response))?;
    Ok(())
}
fn publish_diagnostics(
    connection: &lsp_server::Connection,
    diagnostics: <lsp_types::notification::PublishDiagnostics as lsp_types::notification::Notification>::Params,
) {
    let Ok(diagnostics_json) = serde_json::to_value(diagnostics).map_err(|err| {
        eprintln!("failed to encode diagnostics {err}");
    }) else {
        return;
    };
    connection.sender.send(lsp_server::Message::Notification(
        lsp_server::Notification {
            method: <lsp_types::notification::PublishDiagnostics as lsp_types::notification::Notification>::METHOD.to_string(),
            params: diagnostics_json,
        },
    )).unwrap_or_else(|err| {
        eprintln!("failed to send diagnostics {err}");
    });
}

fn update_state_on_did_change_text_document(
    state: &mut State,
    connection: &lsp_server::Connection,
    did_change_text_document: lsp_types::DidChangeTextDocumentParams,
) {
    let Ok(changed_file_path) = did_change_text_document.text_document.uri.to_file_path() else {
        return;
    };
    if let Some(project_state) = state.projects.get_mut(&changed_file_path) {
        for change in did_change_text_document.content_changes {
            match (change.range, change.range_length) {
                (None, None) => {
                    // means full replacement
                    *project_state = initialize_project_state_from_source(
                        connection,
                        did_change_text_document.text_document.uri,
                        change.text,
                    );
                    return;
                }
                (Some(range), Some(range_length)) => {
                    string_replace_lsp_range(
                        &mut project_state.source,
                        range,
                        range_length as usize,
                        &change.text,
                    );
                }
                (None, _) | (_, None) => {}
            }
        }
        project_state.syntax = parse_still_syntax_project(&project_state.source);
    }
}

#[derive(Debug)]
struct FileInternalCompileProblem {
    title: Box<str>,
    range: lsp_types::Range,
    message_markdown: String,
}

fn initialize_projects_state_for_workspace_directories_into(
    connection: &lsp_server::Connection,
    initialize_arguments: &lsp_types::InitializeParams,
) -> std::collections::HashMap<std::path::PathBuf, ProjectState> {
    let mut projects_state: std::collections::HashMap<std::path::PathBuf, ProjectState> =
        std::collections::HashMap::new();
    let workspace_directory_paths = initialize_arguments
        .workspace_folders
        .iter()
        .flatten()
        .filter_map(|workspace_folder| workspace_folder.uri.to_file_path().ok());
    for project_path in list_still_projects_in_directory_path(workspace_directory_paths) {
        initialize_state_for_project_into(&mut projects_state, project_path);
    }
    let (fully_initialized_project_sender, fully_initialized_project_receiver) =
        std::sync::mpsc::channel();
    std::thread::scope(|thread_scope| {
        for uninitialized_path in projects_state.keys() {
            let projects_that_finished_full_sender = fully_initialized_project_sender.clone();
            thread_scope.spawn(move || {
                let initialized_project_state: ProjectState =
                    initialize_project(connection, uninitialized_path);
                projects_that_finished_full_sender
                    .send((uninitialized_path.clone(), initialized_project_state))
            });
        }
    });
    drop(fully_initialized_project_sender);
    while let Ok((fully_initialized_project_path, fully_parsed_projects)) =
        fully_initialized_project_receiver.recv()
    {
        projects_state.insert(fully_initialized_project_path, fully_parsed_projects);
    }
    projects_state
}
fn initialize_project(
    connection: &lsp_server::Connection,
    path: &std::path::PathBuf,
) -> ProjectState {
    if let Ok(url) = lsp_types::Url::from_file_path(path)
        && let Ok(project_source) = std::fs::read_to_string(path)
    {
        initialize_project_state_from_source(connection, url, project_source)
    } else {
        uninitialized_project_state
    }
}

fn initialize_state_for_project_into(
    projects_state: &mut std::collections::HashMap<std::path::PathBuf, ProjectState>,
    project_path: std::path::PathBuf,
) {
    projects_state.insert(project_path, uninitialized_project_state);
}
/// A yet to be initialized dummy [`ProjectState`]
const uninitialized_project_state: ProjectState = ProjectState {
    source: String::new(),
    syntax: StillSyntaxProject {
        comments: vec![],
        declarations: vec![],
    },
    problems: vec![],
};
fn initialize_project_state_from_source(
    connection: &lsp_server::Connection,
    url: lsp_types::Url,
    source: String,
) -> ProjectState {
    let problems = vec![]; // TODO
    publish_diagnostics(
        connection,
        lsp_types::PublishDiagnosticsParams {
            uri: url,
            diagnostics: problems
                .iter()
                .map(still_make_file_problem_to_diagnostic)
                .collect::<Vec<_>>(),
            version: None,
        },
    );
    ProjectState {
        syntax: parse_still_syntax_project(&source),
        source: source,
        problems: problems,
    }
}

fn state_get_project_by_lsp_url<'a>(
    state: &'a State,
    uri: &lsp_types::Url,
) -> Option<&'a ProjectState> {
    let file_path: std::path::PathBuf = uri.to_file_path().ok()?;
    state.projects.get(&file_path)
}

type StillName = compact_str::CompactString;

fn respond_to_hover(
    state: &State,
    hover_arguments: &lsp_types::HoverParams,
) -> Option<lsp_types::Hover> {
    let hovered_project_state = state_get_project_by_lsp_url(
        state,
        &hover_arguments
            .text_document_position_params
            .text_document
            .uri,
    )?;
    let hovered_symbol_node: StillSyntaxNode<StillSyntaxSymbol> =
        still_syntax_project_find_symbol_at_position(
            &hovered_project_state.syntax,
            hover_arguments.text_document_position_params.position,
        )?;
    match hovered_symbol_node.value {
        StillSyntaxSymbol::TypeVariable { .. } => None,
        StillSyntaxSymbol::ProjectMemberDeclarationName {
            name: hovered_declaration_name,
            documentation,
            declaration: declaration_node,
        } => {
            let origin_declaration_info_markdown: String = match &declaration_node.value {
                StillSyntaxDeclaration::ChoiceType {
                    name: origin_project_declaration_name,
                    parameters: origin_project_declaration_parameters,
                    equals_key_symbol_range: _,
                    variant0_name: origin_project_declaration_variant0_name_node,
                    variant0_value: origin_project_declaration_variant0_maybe_value,
                    variant1_up: origin_project_declaration_variant1_up,
                } => {
                    format!(
                        "{}{}",
                        if Some(hovered_declaration_name)
                            == origin_project_declaration_name
                                .as_ref()
                                .map(|node| node.value.as_ref())
                        {
                            ""
                        } else {
                            "variant in\n"
                        },
                        &present_choice_type_declaration_info_markdown(
                            &hovered_project_state.syntax.comments,
                            declaration_node.range,
                            origin_project_declaration_name
                                .as_ref()
                                .map(|n| still_syntax_node_as_ref_map(n, StillName::as_str)),
                            documentation,
                            origin_project_declaration_parameters,
                            origin_project_declaration_variant0_name_node
                                .as_ref()
                                .map(|n| still_syntax_node_as_ref_map(n, StillName::as_str)),
                            origin_project_declaration_variant0_maybe_value
                                .as_ref()
                                .map(still_syntax_node_as_ref),
                            origin_project_declaration_variant1_up,
                        )
                    )
                }
                StillSyntaxDeclaration::TypeAlias {
                    alias_keyword_range: _,
                    name: maybe_declaration_name,
                    parameters: origin_project_declaration_parameters,
                    equals_key_symbol_range: _,
                    type_,
                } => present_type_alias_declaration_info_markdown(
                    &hovered_project_state.syntax.comments,
                    declaration_node.range,
                    maybe_declaration_name
                        .as_ref()
                        .map(|n| still_syntax_node_as_ref_map(n, StillName::as_str)),
                    documentation,
                    origin_project_declaration_parameters,
                    type_.as_ref().map(still_syntax_node_as_ref),
                ),
                StillSyntaxDeclaration::Variable {
                    start_name: origin_project_declaration_name_node,
                    result: maybe_result_node,
                } => present_variable_declaration_info_markdown(
                    still_syntax_node_as_ref_map(
                        origin_project_declaration_name_node,
                        StillName::as_str,
                    ),
                    documentation,
                    maybe_result_node
                        .as_ref()
                        .and_then(|result_node| {
                            still_syntax_expression_type(still_syntax_node_as_ref(result_node)).ok()
                        })
                        .as_ref()
                        .map(still_syntax_node_as_ref),
                ),
            };
            Some(lsp_types::Hover {
                contents: lsp_types::HoverContents::Markup(lsp_types::MarkupContent {
                    kind: lsp_types::MarkupKind::Markdown,
                    value: origin_declaration_info_markdown,
                }),
                range: Some(hovered_symbol_node.range),
            })
        }
        StillSyntaxSymbol::LetDeclarationName {
            name: hovered_name,
            type_type: maybe_type_type,
            start_name_range,
            scope_expression: _,
        } => Some(lsp_types::Hover {
            contents: lsp_types::HoverContents::Markup(lsp_types::MarkupContent {
                kind: lsp_types::MarkupKind::Markdown,
                value: let_declaration_info_markdown(
                    StillSyntaxNode {
                        range: start_name_range,
                        value: hovered_name,
                    },
                    maybe_type_type.as_ref().map(still_syntax_node_as_ref),
                ),
            }),
            range: Some(hovered_symbol_node.range),
        }),
        StillSyntaxSymbol::VariableOrVariant {
            name: hovered_name,
            local_bindings,
        } => {
            if let Some((hovered_local_binding_origin, _)) =
                find_local_binding_scope_expression(&local_bindings, hovered_name)
            {
                return Some(lsp_types::Hover {
                    contents: lsp_types::HoverContents::Markup(lsp_types::MarkupContent {
                        kind: lsp_types::MarkupKind::Markdown,
                        value: local_binding_info_markdown(
                            hovered_name,
                            hovered_local_binding_origin,
                        ),
                    }),
                    range: Some(hovered_symbol_node.range),
                });
            }
            let origin_declaration_info_markdown: String = hovered_project_state
                .syntax
                .declarations
                .iter()
                .find_map(|origin_project_declaration_or_err| {
                    let origin_project_declaration =
                        origin_project_declaration_or_err.as_ref().ok()?;
                    let origin_project_declaration_node =
                        origin_project_declaration.declaration.as_ref()?;
                    match &origin_project_declaration_node.value {
                        StillSyntaxDeclaration::ChoiceType {
                            name: origin_project_declaration_name,
                            parameters: origin_project_declaration_parameters,
                            equals_key_symbol_range: _,
                            variant0_name: origin_project_declaration_variant0_name_node,
                            variant0_value: origin_project_declaration_variant0_maybe_value,
                            variant1_up: origin_project_declaration_variant1_up,
                        } => {
                            let any_declared_name_matches_hovered: bool =
                                (origin_project_declaration_variant0_name_node
                                    .as_ref()
                                    .is_some_and(|name_node| {
                                        name_node.value.as_str() == hovered_name
                                    }))
                                    || (origin_project_declaration_variant1_up.iter().any(
                                        |variant| {
                                            variant.name.as_ref().is_some_and(|name_node| {
                                                name_node.value.as_str() == hovered_name
                                            })
                                        },
                                    ));
                            if any_declared_name_matches_hovered {
                                Some(format!(
                                    "variant in\n{}",
                                    &present_choice_type_declaration_info_markdown(
                                        &hovered_project_state.syntax.comments,
                                        origin_project_declaration_node.range,
                                        origin_project_declaration_name.as_ref().map(|n| {
                                            still_syntax_node_as_ref_map(n, StillName::as_str)
                                        }),
                                        origin_project_declaration
                                            .documentation
                                            .as_ref()
                                            .map(|node| node.value.as_ref()),
                                        origin_project_declaration_parameters,
                                        origin_project_declaration_variant0_name_node.as_ref().map(
                                            |n| still_syntax_node_as_ref_map(n, StillName::as_str)
                                        ),
                                        origin_project_declaration_variant0_maybe_value
                                            .as_ref()
                                            .map(still_syntax_node_as_ref),
                                        origin_project_declaration_variant1_up,
                                    )
                                ))
                            } else {
                                None
                            }
                        }
                        StillSyntaxDeclaration::TypeAlias {
                            alias_keyword_range: _,
                            name: maybe_origin_project_declaration_name,
                            parameters: origin_project_declaration_parameters,
                            equals_key_symbol_range: _,
                            type_,
                        } => {
                            if let Some(origin_project_declaration_name_node) =
                                maybe_origin_project_declaration_name
                                && origin_project_declaration_name_node.value.as_str()
                                    == hovered_name
                            {
                                Some(format!(
                                    "constructor function for record\n{}",
                                    &present_type_alias_declaration_info_markdown(
                                        &hovered_project_state.syntax.comments,
                                        origin_project_declaration_node.range,
                                        Some(still_syntax_node_as_ref_map(
                                            origin_project_declaration_name_node,
                                            StillName::as_str
                                        )),
                                        origin_project_declaration
                                            .documentation
                                            .as_ref()
                                            .map(|node| node.value.as_ref()),
                                        origin_project_declaration_parameters,
                                        type_.as_ref().map(still_syntax_node_as_ref)
                                    )
                                ))
                            } else {
                                None
                            }
                        }
                        StillSyntaxDeclaration::Variable {
                            start_name: origin_project_declaration_name_node,
                            result: maybe_result_node,
                        } => {
                            if origin_project_declaration_name_node.value.as_str() == hovered_name {
                                Some(present_variable_declaration_info_markdown(
                                    still_syntax_node_as_ref_map(
                                        origin_project_declaration_name_node,
                                        StillName::as_str,
                                    ),
                                    origin_project_declaration
                                        .documentation
                                        .as_ref()
                                        .map(|node| node.value.as_ref()),
                                    maybe_result_node
                                        .as_ref()
                                        .and_then(|result_node| {
                                            still_syntax_expression_type(still_syntax_node_as_ref(
                                                result_node,
                                            ))
                                            .ok()
                                        })
                                        .as_ref()
                                        .map(still_syntax_node_as_ref),
                                ))
                            } else {
                                None
                            }
                        }
                    }
                })?;
            Some(lsp_types::Hover {
                contents: lsp_types::HoverContents::Markup(lsp_types::MarkupContent {
                    kind: lsp_types::MarkupKind::Markdown,
                    value: origin_declaration_info_markdown,
                }),
                range: Some(hovered_symbol_node.range),
            })
        }
        StillSyntaxSymbol::Type { name: hovered_name } => {
            let info_markdown: String = hovered_project_state.syntax.declarations.iter().find_map(
                |origin_project_declaration_or_err| {
                    let origin_project_declaration =
                        origin_project_declaration_or_err.as_ref().ok()?;
                    let origin_project_declaration_node =
                        origin_project_declaration.declaration.as_ref()?;
                    match &origin_project_declaration_node.value {
                        StillSyntaxDeclaration::ChoiceType {
                            name: maybe_origin_project_declaration_name,
                            parameters: origin_project_declaration_parameters,
                            equals_key_symbol_range: _,
                            variant0_name: maybe_origin_project_declaration_variant0_name_node,
                            variant0_value: maybe_origin_project_declaration_variant0_maybe_value,
                            variant1_up: origin_project_declaration_variant1_up,
                        } => {
                            if let Some(origin_project_declaration_name_node) =
                                maybe_origin_project_declaration_name
                                && origin_project_declaration_name_node.value.as_str()
                                    == hovered_name
                            {
                                Some(present_choice_type_declaration_info_markdown(
                                    &hovered_project_state.syntax.comments,
                                    origin_project_declaration_node.range,
                                    Some(still_syntax_node_as_ref_map(
                                        origin_project_declaration_name_node,
                                        StillName::as_str,
                                    )),
                                    origin_project_declaration
                                        .documentation
                                        .as_ref()
                                        .map(|node| node.value.as_ref()),
                                    origin_project_declaration_parameters,
                                    maybe_origin_project_declaration_variant0_name_node
                                        .as_ref()
                                        .map(|n| {
                                            still_syntax_node_as_ref_map(n, StillName::as_str)
                                        }),
                                    maybe_origin_project_declaration_variant0_maybe_value
                                        .as_ref()
                                        .map(still_syntax_node_as_ref),
                                    origin_project_declaration_variant1_up,
                                ))
                            } else {
                                None
                            }
                        }
                        StillSyntaxDeclaration::TypeAlias {
                            alias_keyword_range: _,
                            name: maybe_origin_project_declaration_name,
                            parameters: origin_project_declaration_parameters,
                            equals_key_symbol_range: _,
                            type_,
                        } => {
                            if let Some(origin_project_declaration_name_node) =
                                maybe_origin_project_declaration_name
                                && origin_project_declaration_name_node.value.as_str()
                                    == hovered_name
                            {
                                Some(present_type_alias_declaration_info_markdown(
                                    &hovered_project_state.syntax.comments,
                                    origin_project_declaration_node.range,
                                    Some(still_syntax_node_as_ref_map(
                                        origin_project_declaration_name_node,
                                        StillName::as_str,
                                    )),
                                    origin_project_declaration
                                        .documentation
                                        .as_ref()
                                        .map(|node| node.value.as_ref()),
                                    origin_project_declaration_parameters,
                                    type_.as_ref().map(still_syntax_node_as_ref),
                                ))
                            } else {
                                None
                            }
                        }
                        StillSyntaxDeclaration::Variable { .. } => None,
                    }
                },
            )?;
            Some(lsp_types::Hover {
                contents: lsp_types::HoverContents::Markup(lsp_types::MarkupContent {
                    kind: lsp_types::MarkupKind::Markdown,
                    value: info_markdown,
                }),
                range: Some(hovered_symbol_node.range),
            })
        }
    }
}

fn local_binding_info_markdown(binding_name: &str, binding_origin: LocalBindingOrigin) -> String {
    match binding_origin {
        LocalBindingOrigin::PatternVariable(_) => "variable introduced in pattern".to_string(),
        LocalBindingOrigin::LetDeclaredVariable {
            type_: maybe_type,
            name_range: start_name_range,
        } => let_declaration_info_markdown(
            StillSyntaxNode {
                value: binding_name,
                range: start_name_range,
            },
            maybe_type.as_ref().map(still_syntax_node_as_ref),
        ),
    }
}
fn let_declaration_info_markdown(
    start_name_node: StillSyntaxNode<&str>,
    maybe_type_type: Option<StillSyntaxNode<&StillSyntaxType>>,
) -> String {
    match maybe_type_type {
        None => {
            format!("```still\nlet {}\n```\n", start_name_node.value)
        }
        Some(hovered_local_binding_type) => {
            format!(
                "```still\nlet {} :{}{}\n```\n",
                start_name_node.value,
                match still_syntax_range_line_span(
                    lsp_types::Range {
                        start: start_name_node.range.end,
                        end: hovered_local_binding_type.range.end
                    },
                    &[]
                ) {
                    LineSpan::Single => " ",
                    LineSpan::Multiple => "\n    ",
                },
                &still_syntax_type_to_string(hovered_local_binding_type, 4, &[])
            )
        }
    }
}

fn respond_to_goto_definition(
    state: &State,
    goto_definition_arguments: lsp_types::GotoDefinitionParams,
) -> Option<lsp_types::GotoDefinitionResponse> {
    let goto_symbol_project_state = state_get_project_by_lsp_url(
        state,
        &goto_definition_arguments
            .text_document_position_params
            .text_document
            .uri,
    )?;
    let goto_symbol_node: StillSyntaxNode<StillSyntaxSymbol> =
        still_syntax_project_find_symbol_at_position(
            &goto_symbol_project_state.syntax,
            goto_definition_arguments
                .text_document_position_params
                .position,
        )?;
    match goto_symbol_node.value {
        StillSyntaxSymbol::LetDeclarationName { .. }
        | StillSyntaxSymbol::ProjectMemberDeclarationName { .. } => {
            // already at definition
            None
        }
        StillSyntaxSymbol::TypeVariable {
            scope_declaration,
            name: goto_type_variable_name,
        } => {
            match scope_declaration {
                StillSyntaxDeclaration::ChoiceType {
                    name: _,
                    parameters: origin_type_parameters,
                    equals_key_symbol_range: _,
                    variant0_name: _,
                    variant0_value: _,
                    variant1_up: _,
                } => {
                    let goto_type_variable_name_origin_parameter_node = origin_type_parameters
                        .iter()
                        .find(|origin_choice_type_parameter| {
                            origin_choice_type_parameter.value.as_str() == goto_type_variable_name
                        })?;
                    Some(lsp_types::GotoDefinitionResponse::Scalar(
                        lsp_types::Location {
                            uri: goto_definition_arguments
                                .text_document_position_params
                                .text_document
                                .uri,
                            range: goto_type_variable_name_origin_parameter_node.range,
                        },
                    ))
                }
                StillSyntaxDeclaration::TypeAlias {
                    alias_keyword_range: _,
                    name: _,
                    parameters: origin_type_parameters,
                    equals_key_symbol_range: _,
                    type_: _,
                } => {
                    let goto_type_variable_name_origin_parameter_node = origin_type_parameters
                        .iter()
                        .find(|origin_choice_type_parameter| {
                            origin_choice_type_parameter.value.as_str() == goto_type_variable_name
                        })?;
                    Some(lsp_types::GotoDefinitionResponse::Scalar(
                        lsp_types::Location {
                            uri: goto_definition_arguments
                                .text_document_position_params
                                .text_document
                                .uri,
                            range: goto_type_variable_name_origin_parameter_node.range,
                        },
                    ))
                }
                StillSyntaxDeclaration::Variable { .. } => None,
            }
        }
        StillSyntaxSymbol::VariableOrVariant {
            name: goto_name,
            local_bindings,
        } => {
            if let Some((goto_local_binding_origin, _)) =
                find_local_binding_scope_expression(&local_bindings, goto_name)
            {
                return Some(lsp_types::GotoDefinitionResponse::Scalar(
                    lsp_types::Location {
                        uri: goto_definition_arguments
                            .text_document_position_params
                            .text_document
                            .uri,
                        range: match goto_local_binding_origin {
                            LocalBindingOrigin::PatternVariable(range) => range,
                            LocalBindingOrigin::LetDeclaredVariable {
                                type_: _,
                                name_range: start_name_range,
                            } => start_name_range,
                        },
                    },
                ));
            }
            let declaration_name_range: lsp_types::Range = goto_symbol_project_state
                .syntax
                .declarations
                .iter()
                .find_map(|origin_project_declaration_or_err| {
                    let origin_project_declaration =
                        origin_project_declaration_or_err.as_ref().ok()?;
                    let origin_project_declaration_node =
                        origin_project_declaration.declaration.as_ref()?;
                    match &origin_project_declaration_node.value {
                        StillSyntaxDeclaration::ChoiceType {
                            variant0_name: maybe_origin_project_declaration_variant0_name_node,
                            variant1_up: origin_project_declaration_variant1_up,
                            ..
                        } => {
                            if let Some(origin_project_declaration_variant0_name_node) =
                                maybe_origin_project_declaration_variant0_name_node
                                && origin_project_declaration_variant0_name_node.value.as_str()
                                    == goto_name
                            {
                                Some(origin_project_declaration_variant0_name_node.range)
                            } else {
                                origin_project_declaration_variant1_up
                                    .iter()
                                    .find_map(|variant| {
                                        variant.name.as_ref().and_then(|variant_name_node| {
                                            if variant_name_node.value.as_str() == goto_name {
                                                Some(variant_name_node.range)
                                            } else {
                                                None
                                            }
                                        })
                                    })
                            }
                        }
                        StillSyntaxDeclaration::TypeAlias {
                            name: maybe_origin_project_declaration_name,
                            ..
                        } => {
                            // record type alias constructor function
                            if let Some(origin_project_declaration_name_node) =
                                maybe_origin_project_declaration_name
                                && origin_project_declaration_name_node.value.as_str() == goto_name
                            {
                                Some(origin_project_declaration_name_node.range)
                            } else {
                                None
                            }
                        }
                        StillSyntaxDeclaration::Variable {
                            start_name: origin_project_declaration_name_node,
                            ..
                        } => {
                            if origin_project_declaration_name_node.value.as_str() == goto_name {
                                Some(origin_project_declaration_name_node.range)
                            } else {
                                None
                            }
                        }
                    }
                })?;
            Some(lsp_types::GotoDefinitionResponse::Scalar(
                lsp_types::Location {
                    uri: goto_definition_arguments
                        .text_document_position_params
                        .text_document
                        .uri
                        .clone(),
                    range: declaration_name_range,
                },
            ))
        }
        StillSyntaxSymbol::Type { name: goto_name } => {
            let declaration_name_range: lsp_types::Range = goto_symbol_project_state
                .syntax
                .declarations
                .iter()
                .find_map(|origin_project_declaration_or_err| {
                    let origin_project_declaration =
                        origin_project_declaration_or_err.as_ref().ok()?;
                    let origin_project_declaration_node =
                        origin_project_declaration.declaration.as_ref()?;
                    match &origin_project_declaration_node.value {
                        StillSyntaxDeclaration::ChoiceType {
                            name: maybe_origin_project_declaration_name,
                            ..
                        }
                        | StillSyntaxDeclaration::TypeAlias {
                            name: maybe_origin_project_declaration_name,
                            ..
                        } => {
                            if let Some(origin_project_declaration_name_node) =
                                maybe_origin_project_declaration_name
                                && origin_project_declaration_name_node.value.as_str() == goto_name
                            {
                                Some(origin_project_declaration_name_node.range)
                            } else {
                                None
                            }
                        }
                        StillSyntaxDeclaration::Variable { .. } => None,
                    }
                })?;
            Some(lsp_types::GotoDefinitionResponse::Scalar(
                lsp_types::Location {
                    uri: goto_definition_arguments
                        .text_document_position_params
                        .text_document
                        .uri
                        .clone(),
                    range: declaration_name_range,
                },
            ))
        }
    }
}

fn respond_to_prepare_rename(
    state: &State,
    prepare_rename_arguments: &lsp_types::TextDocumentPositionParams,
) -> Option<Result<lsp_types::PrepareRenameResponse, lsp_server::ResponseError>> {
    let project_state =
        state_get_project_by_lsp_url(state, &prepare_rename_arguments.text_document.uri)?;
    let prepare_rename_symbol_node: StillSyntaxNode<StillSyntaxSymbol> =
        still_syntax_project_find_symbol_at_position(
            &project_state.syntax,
            prepare_rename_arguments.position,
        )?;
    Some(match prepare_rename_symbol_node.value {
        StillSyntaxSymbol::ProjectMemberDeclarationName {
            name,
            declaration: _,
            documentation: _,
        }
        | StillSyntaxSymbol::LetDeclarationName {
            name,
            type_type: _,
            start_name_range: _,
            scope_expression: _,
        }
        | StillSyntaxSymbol::TypeVariable {
            scope_declaration: _,
            name,
        } => Ok(lsp_types::PrepareRenameResponse::RangeWithPlaceholder {
            range: prepare_rename_symbol_node.range,
            placeholder: name.to_string(),
        }),
        StillSyntaxSymbol::VariableOrVariant {
            name,
            local_bindings,
        } => match find_local_binding_scope_expression(&local_bindings, name) {
            Some(_) => Ok(lsp_types::PrepareRenameResponse::RangeWithPlaceholder {
                range: prepare_rename_symbol_node.range,
                placeholder: name.to_string(),
            }),
            None => Ok(lsp_types::PrepareRenameResponse::RangeWithPlaceholder {
                range: lsp_types::Range {
                    start: lsp_position_add_characters(
                        prepare_rename_symbol_node.range.end,
                        -(name.len() as i32),
                    ),
                    end: prepare_rename_symbol_node.range.end,
                },
                placeholder: name.to_string(),
            }),
        },
        StillSyntaxSymbol::Type { name } => {
            Ok(lsp_types::PrepareRenameResponse::RangeWithPlaceholder {
                range: lsp_types::Range {
                    start: lsp_position_add_characters(
                        prepare_rename_symbol_node.range.end,
                        -(name.len() as i32),
                    ),
                    end: prepare_rename_symbol_node.range.end,
                },
                placeholder: name.to_string(),
            })
        }
    })
}

struct ProjectProjectOriginAndState<'a> {
    project_state: &'a ProjectState,
    project_path: &'a std::path::PathBuf,
}
/// TODO inline
fn state_iter_all_projects<'a>(
    state: &'a State,
) -> impl Iterator<Item = ProjectProjectOriginAndState<'a>> {
    state
        .projects
        .iter()
        .map(|(path, state)| ProjectProjectOriginAndState {
            project_path: path,
            project_state: state,
        })
}

fn respond_to_rename(
    state: &State,
    rename_arguments: lsp_types::RenameParams,
) -> Option<Vec<lsp_types::TextDocumentEdit>> {
    let to_rename_project_state = state_get_project_by_lsp_url(
        state,
        &rename_arguments.text_document_position.text_document.uri,
    )?;
    let symbol_to_rename_node: StillSyntaxNode<StillSyntaxSymbol> =
        still_syntax_project_find_symbol_at_position(
            &to_rename_project_state.syntax,
            rename_arguments.text_document_position.position,
        )?;
    Some(match symbol_to_rename_node.value {
        StillSyntaxSymbol::TypeVariable {
            scope_declaration,
            name: type_variable_to_rename,
        } => {
            let mut all_uses_of_renamed_type_variable: Vec<lsp_types::Range> = Vec::new();
            still_syntax_declaration_uses_of_variable_into(
                &mut all_uses_of_renamed_type_variable,
                scope_declaration,
                StillSymbolToReference::TypeVariable(type_variable_to_rename),
            );
            vec![lsp_types::TextDocumentEdit {
                text_document: lsp_types::OptionalVersionedTextDocumentIdentifier {
                    uri: rename_arguments.text_document_position.text_document.uri,
                    version: None,
                },
                edits: all_uses_of_renamed_type_variable
                    .into_iter()
                    .map(|use_range_of_renamed_project| {
                        lsp_types::OneOf::Left(lsp_types::TextEdit {
                            range: use_range_of_renamed_project,
                            new_text: rename_arguments.new_name.clone(),
                        })
                    })
                    .collect::<Vec<_>>(),
            }]
        }
        StillSyntaxSymbol::ProjectMemberDeclarationName {
            name: to_rename_declaration_name,
            documentation: _,
            declaration: _,
        } => {
            let still_declared_symbol_to_rename: StillSymbolToReference =
                if to_rename_declaration_name.starts_with(|c: char| c.is_ascii_uppercase()) {
                    StillSymbolToReference::Type {
                        name: to_rename_declaration_name,
                        including_declaration_name: true,
                    }
                } else {
                    StillSymbolToReference::VariableOrVariant {
                        name: to_rename_declaration_name,
                        including_declaration_name: true,
                    }
                };
            state_iter_all_projects(state)
                .filter_map(move |project| {
                    let mut all_uses_of_at_docs_project_member: Vec<lsp_types::Range> = Vec::new();
                    still_syntax_project_uses_of_variable_into(
                        &mut all_uses_of_at_docs_project_member,
                        &project.project_state.syntax,
                        still_declared_symbol_to_rename,
                    );
                    let still_project_uri: lsp_types::Url =
                        lsp_types::Url::from_file_path(project.project_path).ok()?;
                    Some(lsp_types::TextDocumentEdit {
                        text_document: lsp_types::OptionalVersionedTextDocumentIdentifier {
                            uri: still_project_uri,
                            version: None,
                        },
                        edits: all_uses_of_at_docs_project_member
                            .into_iter()
                            .map(|use_range_of_renamed_project| {
                                lsp_types::OneOf::Left(lsp_types::TextEdit {
                                    range: use_range_of_renamed_project,
                                    new_text: rename_arguments.new_name.clone(),
                                })
                            })
                            .collect::<Vec<_>>(),
                    })
                })
                .collect::<Vec<_>>()
        }
        StillSyntaxSymbol::LetDeclarationName {
            name: to_rename_name,
            start_name_range,
            type_type: _,
            scope_expression,
        } => {
            let mut all_uses_of_let_declaration_to_rename: Vec<lsp_types::Range> = Vec::new();
            still_syntax_expression_uses_of_variable_into(
                &mut all_uses_of_let_declaration_to_rename,
                &[StillLocalBinding {
                    name: to_rename_name,
                    origin: LocalBindingOrigin::LetDeclaredVariable {
                        type_: None, // irrelevant fir finding uses
                        name_range: start_name_range,
                    },
                }],
                scope_expression,
                StillSymbolToReference::LocalBinding {
                    name: to_rename_name,
                    including_let_declaration_name: true,
                },
            );
            vec![lsp_types::TextDocumentEdit {
                text_document: lsp_types::OptionalVersionedTextDocumentIdentifier {
                    uri: rename_arguments.text_document_position.text_document.uri,
                    version: None,
                },
                edits: all_uses_of_let_declaration_to_rename
                    .into_iter()
                    .map(|use_range_of_renamed_project| {
                        lsp_types::OneOf::Left(lsp_types::TextEdit {
                            range: use_range_of_renamed_project,
                            new_text: rename_arguments.new_name.clone(),
                        })
                    })
                    .collect::<Vec<_>>(),
            }]
        }
        StillSyntaxSymbol::VariableOrVariant {
            name: to_rename_name,
            local_bindings,
        } => {
            if let Some((
                to_rename_local_binding_origin,
                local_binding_to_rename_scope_expression,
            )) = find_local_binding_scope_expression(&local_bindings, to_rename_name)
            {
                let mut all_uses_of_local_binding_to_rename: Vec<lsp_types::Range> = Vec::new();
                match to_rename_local_binding_origin {
                    LocalBindingOrigin::PatternVariable(range) => {
                        all_uses_of_local_binding_to_rename.push(range);
                    }
                    LocalBindingOrigin::LetDeclaredVariable { .. } => {
                        // already included in scope expression
                    }
                }
                still_syntax_expression_uses_of_variable_into(
                    &mut all_uses_of_local_binding_to_rename,
                    &[StillLocalBinding {
                        name: to_rename_name,
                        origin: to_rename_local_binding_origin,
                    }],
                    local_binding_to_rename_scope_expression,
                    StillSymbolToReference::LocalBinding {
                        name: to_rename_name,
                        including_let_declaration_name: true,
                    },
                );
                vec![lsp_types::TextDocumentEdit {
                    text_document: lsp_types::OptionalVersionedTextDocumentIdentifier {
                        uri: rename_arguments.text_document_position.text_document.uri,
                        version: None,
                    },
                    edits: all_uses_of_local_binding_to_rename
                        .into_iter()
                        .map(|use_range_of_renamed_project| {
                            lsp_types::OneOf::Left(lsp_types::TextEdit {
                                range: use_range_of_renamed_project,
                                new_text: rename_arguments.new_name.clone(),
                            })
                        })
                        .collect::<Vec<_>>(),
                }]
            } else {
                let symbol_to_find: StillSymbolToReference =
                    StillSymbolToReference::VariableOrVariant {
                        name: to_rename_name,
                        including_declaration_name: true,
                    };
                state_iter_all_projects(state)
                    .filter_map(|project| {
                        let mut all_uses_of_renamed_variable: Vec<lsp_types::Range> = Vec::new();
                        still_syntax_project_uses_of_variable_into(
                            &mut all_uses_of_renamed_variable,
                            &project.project_state.syntax,
                            symbol_to_find,
                        );
                        let still_project_uri: lsp_types::Url =
                            lsp_types::Url::from_file_path(project.project_path).ok()?;
                        Some(lsp_types::TextDocumentEdit {
                            text_document: lsp_types::OptionalVersionedTextDocumentIdentifier {
                                uri: still_project_uri,
                                version: None,
                            },
                            edits: all_uses_of_renamed_variable
                                .into_iter()
                                .map(|use_range_of_renamed_project| {
                                    lsp_types::OneOf::Left(lsp_types::TextEdit {
                                        range: use_range_of_renamed_project,
                                        new_text: rename_arguments.new_name.clone(),
                                    })
                                })
                                .collect::<Vec<_>>(),
                        })
                    })
                    .collect::<Vec<_>>()
            }
        }
        StillSyntaxSymbol::Type {
            name: type_name_to_rename,
        } => {
            let still_declared_symbol_to_rename: StillSymbolToReference =
                StillSymbolToReference::Type {
                    name: type_name_to_rename,
                    including_declaration_name: true,
                };
            state_iter_all_projects(state)
                .filter_map(|project| {
                    let mut all_uses_of_renamed_type: Vec<lsp_types::Range> = Vec::new();
                    still_syntax_project_uses_of_variable_into(
                        &mut all_uses_of_renamed_type,
                        &project.project_state.syntax,
                        still_declared_symbol_to_rename,
                    );
                    let still_project_uri: lsp_types::Url =
                        lsp_types::Url::from_file_path(project.project_path).ok()?;
                    Some(lsp_types::TextDocumentEdit {
                        text_document: lsp_types::OptionalVersionedTextDocumentIdentifier {
                            uri: still_project_uri,
                            version: None,
                        },
                        edits: all_uses_of_renamed_type
                            .into_iter()
                            .map(|use_range_of_renamed_project| {
                                lsp_types::OneOf::Left(lsp_types::TextEdit {
                                    range: use_range_of_renamed_project,
                                    new_text: rename_arguments.new_name.clone(),
                                })
                            })
                            .collect::<Vec<_>>(),
                    })
                })
                .collect::<Vec<_>>()
        }
    })
}
fn respond_to_references(
    state: &State,
    references_arguments: lsp_types::ReferenceParams,
) -> Option<Vec<lsp_types::Location>> {
    let to_find_project_state = state_get_project_by_lsp_url(
        state,
        &references_arguments
            .text_document_position
            .text_document
            .uri,
    )?;
    let symbol_to_find_node: StillSyntaxNode<StillSyntaxSymbol> =
        still_syntax_project_find_symbol_at_position(
            &to_find_project_state.syntax,
            references_arguments.text_document_position.position,
        )?;
    Some(match symbol_to_find_node.value {
        StillSyntaxSymbol::TypeVariable {
            scope_declaration,
            name: type_variable_to_find,
        } => {
            let mut all_uses_of_found_type_variable: Vec<lsp_types::Range> = Vec::new();
            still_syntax_declaration_uses_of_variable_into(
                &mut all_uses_of_found_type_variable,
                scope_declaration,
                StillSymbolToReference::TypeVariable(type_variable_to_find),
            );
            all_uses_of_found_type_variable
                .into_iter()
                .map(|use_range_of_found_project| lsp_types::Location {
                    uri: references_arguments
                        .text_document_position
                        .text_document
                        .uri
                        .clone(),
                    range: use_range_of_found_project,
                })
                .collect::<Vec<_>>()
        }
        StillSyntaxSymbol::ProjectMemberDeclarationName {
            name: to_find_name,
            documentation: _,
            declaration: _,
        } => {
            let still_declared_symbol_to_find: StillSymbolToReference = if to_find_name
                .starts_with(|c: char| c.is_ascii_uppercase())
            {
                StillSymbolToReference::Type {
                    name: to_find_name,
                    including_declaration_name: references_arguments.context.include_declaration,
                }
            } else {
                StillSymbolToReference::VariableOrVariant {
                    name: to_find_name,
                    including_declaration_name: references_arguments.context.include_declaration,
                }
            };
            let mut all_uses_of_found_project_member: Vec<lsp_types::Range> = Vec::new();
            still_syntax_project_uses_of_variable_into(
                &mut all_uses_of_found_project_member,
                &to_find_project_state.syntax,
                still_declared_symbol_to_find,
            );
            all_uses_of_found_project_member
                .into_iter()
                .map(move |use_range_of_found_project| lsp_types::Location {
                    uri: references_arguments
                        .text_document_position
                        .text_document
                        .uri
                        .clone(),
                    range: use_range_of_found_project,
                })
                .collect::<Vec<_>>()
        }
        StillSyntaxSymbol::LetDeclarationName {
            name: to_find_name,
            start_name_range,
            type_type: _,
            scope_expression,
        } => {
            let mut all_uses_of_found_let_declaration: Vec<lsp_types::Range> = Vec::new();
            still_syntax_expression_uses_of_variable_into(
                &mut all_uses_of_found_let_declaration,
                &[StillLocalBinding {
                    name: to_find_name,
                    origin: LocalBindingOrigin::LetDeclaredVariable {
                        type_: None, // irrelevant for finding uses
                        name_range: start_name_range,
                    },
                }],
                scope_expression,
                StillSymbolToReference::LocalBinding {
                    name: to_find_name,
                    including_let_declaration_name: references_arguments
                        .context
                        .include_declaration,
                },
            );
            all_uses_of_found_let_declaration
                .into_iter()
                .map(|use_range_of_found_project| lsp_types::Location {
                    uri: references_arguments
                        .text_document_position
                        .text_document
                        .uri
                        .clone(),
                    range: use_range_of_found_project,
                })
                .collect::<Vec<_>>()
        }
        StillSyntaxSymbol::VariableOrVariant {
            name: to_find_name,
            local_bindings,
        } => {
            if let Some((to_find_local_binding_origin, local_binding_to_find_scope_expression)) =
                find_local_binding_scope_expression(&local_bindings, to_find_name)
            {
                let mut all_uses_of_found_local_binding: Vec<lsp_types::Range> = Vec::new();
                if references_arguments.context.include_declaration {
                    match to_find_local_binding_origin {
                        LocalBindingOrigin::PatternVariable(range) => {
                            all_uses_of_found_local_binding.push(range);
                        }
                        LocalBindingOrigin::LetDeclaredVariable { .. } => {
                            // already included in scope
                        }
                    }
                }
                still_syntax_expression_uses_of_variable_into(
                    &mut all_uses_of_found_local_binding,
                    &[StillLocalBinding {
                        name: to_find_name,
                        origin: to_find_local_binding_origin,
                    }],
                    local_binding_to_find_scope_expression,
                    StillSymbolToReference::LocalBinding {
                        name: to_find_name,
                        including_let_declaration_name: references_arguments
                            .context
                            .include_declaration,
                    },
                );
                all_uses_of_found_local_binding
                    .into_iter()
                    .map(|use_range_of_found_project| lsp_types::Location {
                        uri: references_arguments
                            .text_document_position
                            .text_document
                            .uri
                            .clone(),
                        range: use_range_of_found_project,
                    })
                    .collect::<Vec<_>>()
            } else {
                let symbol_to_find: StillSymbolToReference =
                    StillSymbolToReference::VariableOrVariant {
                        name: to_find_name,
                        including_declaration_name: references_arguments
                            .context
                            .include_declaration,
                    };
                let mut all_uses_of_found_variable: Vec<lsp_types::Range> = Vec::new();
                still_syntax_project_uses_of_variable_into(
                    &mut all_uses_of_found_variable,
                    &to_find_project_state.syntax,
                    symbol_to_find,
                );

                all_uses_of_found_variable
                    .into_iter()
                    .map(move |use_range_of_found_project| lsp_types::Location {
                        uri: references_arguments
                            .text_document_position
                            .text_document
                            .uri
                            .clone(),
                        range: use_range_of_found_project,
                    })
                    .collect::<Vec<_>>()
            }
        }
        StillSyntaxSymbol::Type {
            name: type_name_to_find,
        } => {
            let still_declared_symbol_to_find: StillSymbolToReference =
                StillSymbolToReference::Type {
                    name: type_name_to_find,
                    including_declaration_name: references_arguments.context.include_declaration,
                };
            let mut all_uses_of_found_type: Vec<lsp_types::Range> = Vec::new();
            still_syntax_project_uses_of_variable_into(
                &mut all_uses_of_found_type,
                &to_find_project_state.syntax,
                still_declared_symbol_to_find,
            );

            all_uses_of_found_type
                .into_iter()
                .map(move |use_range_of_found_project| lsp_types::Location {
                    uri: references_arguments
                        .text_document_position
                        .text_document
                        .uri
                        .clone(),
                    range: use_range_of_found_project,
                })
                .collect::<Vec<_>>()
        }
    })
}

fn respond_to_semantic_tokens_full(
    state: &State,
    semantic_tokens_arguments: &lsp_types::SemanticTokensParams,
) -> Option<lsp_types::SemanticTokensResult> {
    let project_state =
        state_get_project_by_lsp_url(state, &semantic_tokens_arguments.text_document.uri)?;
    let mut highlighting: Vec<StillSyntaxNode<StillSyntaxHighlightKind>> =
        Vec::with_capacity(project_state.source.len() / 16);
    still_syntax_highlight_project_into(&mut highlighting, &project_state.syntax);
    Some(lsp_types::SemanticTokensResult::Tokens(
        lsp_types::SemanticTokens {
            result_id: None,
            data: highlighting
                .into_iter()
                .scan(
                    lsp_types::Position {
                        line: 0,
                        character: 0,
                    },
                    |previous_start_location, segment| {
                        if (segment.range.end.line != segment.range.start.line)
                            || (segment.range.end.character < segment.range.start.character)
                        {
                            eprintln!(
                                "bad highlight token range: must be single-line and positive {:?}",
                                segment.range
                            );
                            return None;
                        }
                        match lsp_position_positive_delta(
                            *previous_start_location,
                            segment.range.start,
                        ) {
                            Err(error) => {
                                eprintln!("bad highlight token order {error}");
                                None
                            }
                            Ok(delta) => {
                                let token = lsp_types::SemanticToken {
                                    delta_line: delta.line,
                                    delta_start: delta.character,
                                    length: segment.range.end.character
                                        - segment.range.start.character,
                                    token_type: semantic_token_type_to_id(
                                        &still_syntax_highlight_kind_to_lsp_semantic_token_type(
                                            &segment.value,
                                        ),
                                    ),
                                    token_modifiers_bitset: 0_u32,
                                };
                                segment.range.start.clone_into(previous_start_location);
                                Some(token)
                            }
                        }
                    },
                )
                .collect::<Vec<lsp_types::SemanticToken>>(),
        },
    ))
}

const token_types: [lsp_types::SemanticTokenType; 11] = [
    lsp_types::SemanticTokenType::NUMBER,
    lsp_types::SemanticTokenType::STRING,
    lsp_types::SemanticTokenType::NAMESPACE,
    lsp_types::SemanticTokenType::VARIABLE,
    lsp_types::SemanticTokenType::TYPE,
    lsp_types::SemanticTokenType::TYPE_PARAMETER,
    lsp_types::SemanticTokenType::KEYWORD,
    lsp_types::SemanticTokenType::ENUM_MEMBER,
    lsp_types::SemanticTokenType::PROPERTY,
    lsp_types::SemanticTokenType::COMMENT,
    lsp_types::SemanticTokenType::FUNCTION,
];

fn semantic_token_type_to_id(semantic_token: &lsp_types::SemanticTokenType) -> u32 {
    token_types
        .iter()
        .enumerate()
        .find_map(|(i, token)| {
            if token == semantic_token {
                Some(i as u32)
            } else {
                None
            }
        })
        .unwrap_or(0_u32)
}

fn present_variable_declaration_info_markdown(
    start_name_node: StillSyntaxNode<&str>,
    maybe_documentation: Option<&str>,
    retrieved_type_result: Option<StillSyntaxNode<&StillSyntaxType>>,
) -> String {
    let description: String = match retrieved_type_result {
        Some(retrieved_type) => {
            format!(
                "```still\n{} :{}{}\n```\n",
                start_name_node.value,
                match still_syntax_range_line_span(retrieved_type.range, &[]) {
                    LineSpan::Single => " ",
                    LineSpan::Multiple => "\n    ",
                },
                &still_syntax_type_to_string(retrieved_type, 4, &[])
            )
        }
        None => format!("```still\n{}\n```\n", &start_name_node.value),
    };
    match maybe_documentation {
        None => description,
        Some(documentation) => {
            description + "##-\n" + documentation_comment_to_markdown(documentation).as_str()
        }
    }
}
fn present_type_alias_declaration_info_markdown(
    comments: &[StillSyntaxNode<Box<str>>],
    declaration_range: lsp_types::Range,
    maybe_name: Option<StillSyntaxNode<&str>>,
    maybe_documentation: Option<&str>,
    parameters: &[StillSyntaxNode<StillName>],
    maybe_type: Option<StillSyntaxNode<&StillSyntaxType>>,
) -> String {
    let mut declaration_as_string: String = String::new();
    let maybe_fully_qualified_name: Option<StillSyntaxNode<String>> =
        maybe_name.map(|name_node| still_syntax_node_map(name_node, str::to_string));
    still_syntax_type_alias_declaration_into(
        &mut declaration_as_string,
        still_syntax_comments_in_range(comments, declaration_range),
        declaration_range,
        maybe_fully_qualified_name
            .as_ref()
            .map(|name_node| still_syntax_node_as_ref_map(name_node, String::as_str)),
        parameters,
        maybe_type,
    );
    let description = format!("```still\n{}\n```\n", declaration_as_string);
    match maybe_documentation {
        None => description,
        Some(documentation) => {
            description + "##-\n" + documentation_comment_to_markdown(documentation).as_str()
        }
    }
}

fn present_choice_type_declaration_info_markdown(
    comments: &[StillSyntaxNode<Box<str>>],
    declaration_range: lsp_types::Range,
    maybe_name: Option<StillSyntaxNode<&str>>,
    maybe_documentation: Option<&str>,
    parameters: &[StillSyntaxNode<StillName>],
    variant0_name: Option<StillSyntaxNode<&str>>,
    variant0_maybe_value: Option<StillSyntaxNode<&StillSyntaxType>>,
    variant1_up: &[StillSyntaxChoiceTypeDeclarationTailingVariant],
) -> String {
    let mut declaration_string: String = String::new();
    let maybe_fully_qualified_name: Option<StillSyntaxNode<String>> =
        maybe_name.map(|name_node| still_syntax_node_map(name_node, str::to_string));
    still_syntax_choice_type_declaration_into(
        &mut declaration_string,
        still_syntax_comments_in_range(comments, declaration_range),
        declaration_range,
        maybe_fully_qualified_name
            .as_ref()
            .map(|name_node| still_syntax_node_as_ref_map(name_node, String::as_str)),
        parameters,
        variant0_name,
        variant0_maybe_value,
        variant1_up,
    );
    let description: String = format!("```still\n{}\n```\n", declaration_string);
    match maybe_documentation {
        None => description,
        Some(documentation) => {
            description + "##-\n" + documentation_comment_to_markdown(documentation).as_str()
        }
    }
}

fn respond_to_completion(
    state: &State,
    completion_arguments: &lsp_types::CompletionParams,
) -> Option<lsp_types::CompletionResponse> {
    let completion_project = state_get_project_by_lsp_url(
        state,
        &completion_arguments
            .text_document_position
            .text_document
            .uri,
    )?;
    let symbol_to_complete: StillSyntaxNode<StillSyntaxSymbol> =
        still_syntax_project_find_symbol_at_position(
            &completion_project.syntax,
            completion_arguments.text_document_position.position,
        )?;
    let maybe_completion_items: Option<Vec<lsp_types::CompletionItem>> = match symbol_to_complete
        .value
    {
        StillSyntaxSymbol::LetDeclarationName { .. } => None,
        StillSyntaxSymbol::ProjectMemberDeclarationName { .. } => None,
        StillSyntaxSymbol::VariableOrVariant {
            name: _,
            local_bindings,
        } => {
            let mut completion_items: Vec<lsp_types::CompletionItem> = Vec::new();
            let local_binding_completions = local_bindings
                .into_iter()
                .flat_map(|(_, scope_introduced_bindings)| scope_introduced_bindings.into_iter())
                .map(|local_binding| lsp_types::CompletionItem {
                    label: local_binding.name.to_string(),
                    kind: Some(lsp_types::CompletionItemKind::VARIABLE),
                    documentation: Some(lsp_types::Documentation::MarkupContent(
                        lsp_types::MarkupContent {
                            kind: lsp_types::MarkupKind::Markdown,
                            value: local_binding_info_markdown(
                                local_binding.name,
                                local_binding.origin,
                            ),
                        },
                    )),
                    ..lsp_types::CompletionItem::default()
                });
            completion_items.extend(local_binding_completions);
            variable_declaration_completions_into(
                &completion_project.syntax,
                &mut completion_items,
            );
            Some(completion_items)
        }
        StillSyntaxSymbol::Type { name: _ } => {
            let mut completion_items: Vec<lsp_types::CompletionItem> = Vec::new();
            type_declaration_completions_into(&completion_project.syntax, &mut completion_items);
            Some(completion_items)
        }
        StillSyntaxSymbol::TypeVariable { .. } => {
            // is this ever useful to add? still tends to use single-letter names anyway most of the time
            // (or ones where the first letters don't match in the first place).
            // suggesting completions can get annoying and isn't free computationally so...
            None
        }
    };
    maybe_completion_items.map(lsp_types::CompletionResponse::Array)
}

fn variable_declaration_completions_into(
    project_syntax: &StillSyntaxProject,
    completion_items: &mut Vec<lsp_types::CompletionItem>,
) {
    for (origin_project_declaration_node, origin_project_declaration_documentation) in
        project_syntax
            .declarations
            .iter()
            .filter_map(|declaration_or_err| declaration_or_err.as_ref().ok())
            .filter_map(|documented_declaration| {
                documented_declaration
                    .declaration
                    .as_ref()
                    .map(|declaration_node| {
                        (
                            declaration_node,
                            documented_declaration
                                .documentation
                                .as_ref()
                                .map(|node| node.value.as_ref()),
                        )
                    })
            })
    {
        match &origin_project_declaration_node.value {
            StillSyntaxDeclaration::ChoiceType {
                name: maybe_choice_type_name,
                parameters,
                equals_key_symbol_range: _,
                variant0_name,
                variant0_value: variant0_maybe_value,
                variant1_up,
            } => {
                if let Some(choice_type_name_node) = maybe_choice_type_name {
                    let info_markdown: String = format!(
                        "variant in\n{}",
                        present_choice_type_declaration_info_markdown(
                            &project_syntax.comments,
                            origin_project_declaration_node.range,
                            Some(still_syntax_node_as_ref_map(
                                choice_type_name_node,
                                StillName::as_str
                            )),
                            origin_project_declaration_documentation,
                            parameters,
                            variant0_name
                                .as_ref()
                                .map(|n| still_syntax_node_as_ref_map(n, StillName::as_str)),
                            variant0_maybe_value.as_ref().map(still_syntax_node_as_ref),
                            variant1_up,
                        ),
                    );
                    completion_items.extend(
                        variant0_name
                            .as_ref()
                            .map(|node| node.value.to_string())
                            .into_iter()
                            .chain(variant1_up.iter().filter_map(|variant| {
                                variant.name.as_ref().map(|node| node.value.to_string())
                            }))
                            .map(|variant_name: String| lsp_types::CompletionItem {
                                label: variant_name,
                                kind: Some(lsp_types::CompletionItemKind::ENUM_MEMBER),
                                documentation: Some(lsp_types::Documentation::MarkupContent(
                                    lsp_types::MarkupContent {
                                        kind: lsp_types::MarkupKind::Markdown,
                                        value: info_markdown.clone(),
                                    },
                                )),
                                ..lsp_types::CompletionItem::default()
                            }),
                    );
                }
            }
            StillSyntaxDeclaration::TypeAlias {
                alias_keyword_range: _,
                name: maybe_name,
                parameters,
                equals_key_symbol_range: _,
                type_: maybe_type,
            } => {
                if let Some(name_node) = maybe_name
                    && let Some(type_node) = maybe_type
                    && let StillSyntaxType::Record(_) = type_node.value
                {
                    completion_items.push(lsp_types::CompletionItem {
                        label: name_node.value.to_string(),
                        kind: Some(lsp_types::CompletionItemKind::CONSTRUCTOR),
                        documentation: Some(lsp_types::Documentation::MarkupContent(
                            lsp_types::MarkupContent {
                                kind: lsp_types::MarkupKind::Markdown,
                                value: format!(
                                    "constructor function for record\n{}",
                                    &present_type_alias_declaration_info_markdown(
                                        &project_syntax.comments,
                                        origin_project_declaration_node.range,
                                        Some(still_syntax_node_as_ref_map(
                                            name_node,
                                            StillName::as_str
                                        )),
                                        origin_project_declaration_documentation,
                                        parameters,
                                        Some(still_syntax_node_as_ref(type_node)),
                                    )
                                ),
                            },
                        )),
                        ..lsp_types::CompletionItem::default()
                    });
                }
            }
            StillSyntaxDeclaration::Variable {
                start_name: start_name_node,
                result: maybe_result_node,
            } => {
                completion_items.push(lsp_types::CompletionItem {
                    label: start_name_node.value.to_string(),
                    kind: Some(lsp_types::CompletionItemKind::FUNCTION),
                    documentation: Some(lsp_types::Documentation::MarkupContent(
                        lsp_types::MarkupContent {
                            kind: lsp_types::MarkupKind::Markdown,
                            value: present_variable_declaration_info_markdown(
                                still_syntax_node_as_ref_map(start_name_node, StillName::as_str),
                                origin_project_declaration_documentation,
                                maybe_result_node
                                    .as_ref()
                                    .and_then(|result_node| {
                                        still_syntax_expression_type(still_syntax_node_as_ref(
                                            result_node,
                                        ))
                                        .ok()
                                    })
                                    .as_ref()
                                    .map(still_syntax_node_as_ref),
                            ),
                        },
                    )),
                    ..lsp_types::CompletionItem::default()
                });
            }
        }
    }
}
fn type_declaration_completions_into(
    project_syntax: &StillSyntaxProject,
    completion_items: &mut Vec<lsp_types::CompletionItem>,
) {
    for (origin_project_declaration_node, origin_project_declaration_documentation) in
        project_syntax
            .declarations
            .iter()
            .filter_map(|declaration_or_err| declaration_or_err.as_ref().ok())
            .filter_map(|documented_declaration| {
                documented_declaration
                    .declaration
                    .as_ref()
                    .map(|declaration_node| {
                        (
                            declaration_node,
                            documented_declaration
                                .documentation
                                .as_ref()
                                .map(|node| node.value.as_ref()),
                        )
                    })
            })
    {
        match &origin_project_declaration_node.value {
            StillSyntaxDeclaration::ChoiceType {
                name: maybe_name,
                parameters,
                equals_key_symbol_range: _,
                variant0_name: maybe_variant0_name,
                variant0_value: variant0_maybe_value,
                variant1_up,
            } => {
                if let Some(name_node) = maybe_name.as_ref() {
                    completion_items.push(lsp_types::CompletionItem {
                        label: name_node.value.to_string(),
                        kind: Some(lsp_types::CompletionItemKind::ENUM),
                        documentation: Some(lsp_types::Documentation::MarkupContent(
                            lsp_types::MarkupContent {
                                kind: lsp_types::MarkupKind::Markdown,
                                value: present_choice_type_declaration_info_markdown(
                                    &project_syntax.comments,
                                    origin_project_declaration_node.range,
                                    Some(still_syntax_node_as_ref_map(
                                        name_node,
                                        StillName::as_str,
                                    )),
                                    origin_project_declaration_documentation,
                                    parameters,
                                    maybe_variant0_name.as_ref().map(|n| {
                                        still_syntax_node_as_ref_map(n, StillName::as_str)
                                    }),
                                    variant0_maybe_value.as_ref().map(still_syntax_node_as_ref),
                                    variant1_up,
                                ),
                            },
                        )),
                        ..lsp_types::CompletionItem::default()
                    });
                }
            }
            StillSyntaxDeclaration::TypeAlias {
                alias_keyword_range: _,
                name: maybe_name,
                parameters,
                equals_key_symbol_range: _,
                type_,
            } => {
                if let Some(name_node) = maybe_name.as_ref() {
                    completion_items.push(lsp_types::CompletionItem {
                        label: name_node.value.to_string(),
                        kind: Some(lsp_types::CompletionItemKind::STRUCT),
                        documentation: Some(lsp_types::Documentation::MarkupContent(
                            lsp_types::MarkupContent {
                                kind: lsp_types::MarkupKind::Markdown,
                                value: present_type_alias_declaration_info_markdown(
                                    &project_syntax.comments,
                                    origin_project_declaration_node.range,
                                    Some(still_syntax_node_as_ref_map(
                                        name_node,
                                        StillName::as_str,
                                    )),
                                    origin_project_declaration_documentation,
                                    parameters,
                                    type_.as_ref().map(still_syntax_node_as_ref),
                                ),
                            },
                        )),
                        ..lsp_types::CompletionItem::default()
                    });
                }
            }
            StillSyntaxDeclaration::Variable { .. } => {}
        }
    }
}

fn respond_to_document_formatting(
    state: &State,
    formatting_arguments: &lsp_types::DocumentFormattingParams,
) -> Option<Vec<lsp_types::TextEdit>> {
    let document_path: std::path::PathBuf =
        formatting_arguments.text_document.uri.to_file_path().ok()?;
    let to_format_project = state.projects.get(&document_path)?;
    let formatted: String = still_syntax_project_format(to_format_project);
    // diffing does not seem to be needed here. But maybe it's faster?
    Some(vec![lsp_types::TextEdit {
        range: lsp_types::Range {
            start: lsp_types::Position {
                line: 0,
                character: 0,
            },
            end: lsp_types::Position {
                line: 1_000_000_000, // to_format_project.source.lines().count() as u32 + 1
                character: 0,
            },
        },
        new_text: formatted,
    }])
}

fn respond_to_document_symbols(
    state: &State,
    document_symbol_arguments: &lsp_types::DocumentSymbolParams,
) -> Option<lsp_types::DocumentSymbolResponse> {
    let document_path: std::path::PathBuf = document_symbol_arguments
        .text_document
        .uri
        .to_file_path()
        .ok()?;
    let project = state.projects.get(&document_path)?;
    Some(lsp_types::DocumentSymbolResponse::Nested(
        project
            .syntax
            .declarations
            .iter()
            .filter_map(|declaration_or_err| declaration_or_err.as_ref().ok())
            .filter_map(|documented_declaration| documented_declaration.declaration.as_ref())
            .filter_map(|declaration_node| match &declaration_node.value {
                StillSyntaxDeclaration::ChoiceType {
                    name: maybe_name,
                    parameters: _,
                    equals_key_symbol_range: _,
                    variant0_name,
                    variant0_value: variant0_maybe_value,
                    variant1_up,
                } => {
                    let name_node = maybe_name.as_ref()?;
                    Some(lsp_types::DocumentSymbol {
                        name: name_node.value.to_string(),
                        detail: None,
                        kind: lsp_types::SymbolKind::ENUM,
                        tags: None,
                        #[allow(deprecated)]
                        deprecated: None,
                        range: declaration_node.range,
                        selection_range: name_node.range,
                        children: Some(
                            variant0_name
                                .as_ref()
                                .map(|variant0_name_node| {
                                    (
                                        variant0_name_node,
                                        lsp_types::Range {
                                            start: variant0_name_node.range.start,
                                            end: variant0_maybe_value
                                                .as_ref()
                                                .map(|node| node.range.end)
                                                .unwrap_or(variant0_name_node.range.end),
                                        },
                                    )
                                })
                                .into_iter()
                                .chain(variant1_up.iter().filter_map(|variant| {
                                    let variant_name_node = variant.name.as_ref()?;
                                    Some((
                                        variant_name_node,
                                        lsp_types::Range {
                                            start: variant_name_node.range.start,
                                            end: variant
                                                .value
                                                .as_ref()
                                                .map(|node| node.range.end)
                                                .unwrap_or(variant_name_node.range.end),
                                        },
                                    ))
                                }))
                                .map(|(variant_name_node, variant_full_range)| {
                                    lsp_types::DocumentSymbol {
                                        name: variant_name_node.value.to_string(),
                                        detail: None,
                                        kind: lsp_types::SymbolKind::ENUM_MEMBER,
                                        tags: None,
                                        #[allow(deprecated)]
                                        deprecated: None,
                                        range: variant_full_range,
                                        selection_range: variant_name_node.range,
                                        children: None,
                                    }
                                })
                                .collect::<Vec<_>>(),
                        ),
                    })
                }
                StillSyntaxDeclaration::TypeAlias {
                    name: maybe_name,
                    alias_keyword_range: _,
                    parameters: _,
                    equals_key_symbol_range: _,
                    type_: _,
                } => {
                    let name_node = maybe_name.as_ref()?;
                    Some(lsp_types::DocumentSymbol {
                        name: name_node.value.to_string(),
                        detail: None,
                        kind: lsp_types::SymbolKind::STRUCT,
                        tags: None,
                        #[allow(deprecated)]
                        deprecated: None,
                        range: declaration_node.range,
                        selection_range: name_node.range,
                        children: None,
                    })
                }
                StillSyntaxDeclaration::Variable {
                    start_name: start_name_node,
                    result: _,
                } => Some(lsp_types::DocumentSymbol {
                    name: start_name_node.value.to_string(),
                    detail: None,
                    kind: lsp_types::SymbolKind::FUNCTION,
                    tags: None,
                    #[allow(deprecated)]
                    deprecated: None,
                    range: declaration_node.range,
                    selection_range: start_name_node.range,
                    children: None,
                }),
            })
            .collect::<Vec<_>>(),
    ))
}

fn still_make_file_problem_to_diagnostic(
    problem: &FileInternalCompileProblem,
) -> lsp_types::Diagnostic {
    lsp_types::Diagnostic {
        range: problem.range,
        severity: Some(lsp_types::DiagnosticSeverity::ERROR),
        code: None,
        code_description: None,
        source: None,
        message: format!("#- {} #-\n{}", &problem.title, &problem.message_markdown),
        related_information: None,
        tags: None,
        data: None,
    }
}

fn documentation_comment_to_markdown(documentation: &str) -> String {
    let markdown_source: &str = documentation.trim();
    let mut builder: String = String::new();
    markdown_convert_code_blocks_to_still_into(&mut builder, markdown_source);
    builder
}
fn markdown_convert_code_blocks_to_still_into(builder: &mut String, markdown_source: &str) {
    // because I don't want to introduce a full markdown parser for just this tiny
    // improvement, the code below only approximates where code blocks are.
    let mut with_fenced_code_blocks_converted = String::new();
    markdown_convert_unspecific_fenced_code_blocks_to_still_into(
        &mut with_fenced_code_blocks_converted,
        markdown_source,
    );
    markdown_convert_indented_code_blocks_to_still(builder, &with_fenced_code_blocks_converted);
}

/// replace fenced no-language-specified code blocks by `still...`
fn markdown_convert_unspecific_fenced_code_blocks_to_still_into(
    result_builder: &mut String,
    markdown_source: &str,
) {
    let mut current_source_index: usize = 0;
    'converting_fenced: while current_source_index < markdown_source.len() {
        match markdown_source[current_source_index..]
            .find("```")
            .map(|i| i + current_source_index)
        {
            None => {
                result_builder.push_str(&markdown_source[current_source_index..]);
                break 'converting_fenced;
            }
            Some(index_at_opening_fence) => {
                let index_after_opening_fence = index_at_opening_fence + 3;
                match markdown_source[index_after_opening_fence..]
                    .find("```")
                    .map(|i| i + index_after_opening_fence)
                {
                    None => {
                        result_builder.push_str(&markdown_source[current_source_index..]);
                        break 'converting_fenced;
                    }
                    Some(index_at_closing_fence) => {
                        match markdown_source[index_after_opening_fence..].chars().next() {
                            // fenced block without a specific language
                            Some('\n') => {
                                result_builder.push_str(
                                    &markdown_source[current_source_index..index_at_opening_fence],
                                );
                                result_builder.push_str("```still");
                                result_builder.push_str(
                                    &markdown_source
                                        [index_after_opening_fence..index_at_closing_fence],
                                );
                                result_builder.push_str("```");
                                current_source_index = index_at_closing_fence + 3;
                            }
                            // fenced block with a specific language
                            _ => {
                                result_builder.push_str(
                                    &markdown_source
                                        [current_source_index..(index_at_closing_fence + 3)],
                                );
                                current_source_index = index_at_closing_fence + 3;
                            }
                        }
                    }
                }
            }
        }
    }
}

fn markdown_convert_indented_code_blocks_to_still(builder: &mut String, markdown_source: &str) {
    let mut current_indent: usize = 0;
    let mut is_in_code_block: bool = false;
    let mut previous_line_was_blank: bool = false;
    for source_line in markdown_source.lines() {
        if source_line.is_empty() {
            builder.push('\n');
            previous_line_was_blank = true;
        } else {
            let current_line_indent: usize = source_line
                .chars()
                .take_while(char::is_ascii_whitespace)
                .count();
            if current_line_indent == source_line.len() {
                // ignore blank line
                builder.push_str(source_line);
                builder.push('\n');
                previous_line_was_blank = true;
            } else {
                if is_in_code_block {
                    if current_line_indent <= current_indent - 1 {
                        is_in_code_block = false;
                        current_indent = current_line_indent;
                        builder.push_str("```\n");
                        builder.push_str(source_line);
                        builder.push('\n');
                    } else {
                        builder.push_str(&source_line[current_indent..]);
                        builder.push('\n');
                    }
                } else if previous_line_was_blank && (current_line_indent >= current_indent + 4) {
                    is_in_code_block = true;
                    current_indent = current_line_indent;
                    builder.push_str("```still\n");
                    builder.push_str(&source_line[current_line_indent..]);
                    builder.push('\n');
                } else {
                    current_indent = current_line_indent;
                    builder.push_str(source_line);
                    builder.push('\n');
                }
                previous_line_was_blank = false;
            }
        }
    }
    if is_in_code_block {
        builder.push_str("```\n");
    }
}

fn lsp_range_includes_position(range: lsp_types::Range, position: lsp_types::Position) -> bool {
    (
        // position >= range.start
        (position.line > range.start.line)
            || ((position.line == range.start.line)
                && (position.character >= range.start.character))
    ) && (
        // position <= range.end
        (position.line < range.end.line)
            || ((position.line == range.end.line) && (position.character <= range.end.character))
    )
}

struct PositionDelta {
    line: u32,
    character: u32,
}
fn lsp_position_positive_delta(
    before: lsp_types::Position,
    after: lsp_types::Position,
) -> Result<PositionDelta, String> {
    match before.line.cmp(&after.line) {
        std::cmp::Ordering::Greater => Err(format!(
            "before line > after line (before: {}, after {})",
            lsp_position_to_string(before),
            lsp_position_to_string(after)
        )),
        std::cmp::Ordering::Equal => {
            if before.character > after.character {
                Err(format!(
                    "before character > after character (before: {}, after {})",
                    lsp_position_to_string(before),
                    lsp_position_to_string(after)
                ))
            } else {
                Ok(PositionDelta {
                    line: 0,
                    character: after.character - before.character,
                })
            }
        }
        std::cmp::Ordering::Less => Ok(PositionDelta {
            line: after.line - before.line,
            character: after.character,
        }),
    }
}
fn lsp_position_to_string(lsp_position: lsp_types::Position) -> String {
    format!("{}:{}", lsp_position.line, lsp_position.character)
}

fn lsp_position_add_characters(
    position: lsp_types::Position,
    additional_character_count: i32,
) -> lsp_types::Position {
    lsp_types::Position {
        line: position.line,
        character: (position.character as i32 + additional_character_count) as u32,
    }
}

fn still_syntax_highlight_kind_to_lsp_semantic_token_type(
    still_syntax_highlight_kind: &StillSyntaxHighlightKind,
) -> lsp_types::SemanticTokenType {
    match still_syntax_highlight_kind {
        StillSyntaxHighlightKind::KeySymbol => lsp_types::SemanticTokenType::KEYWORD,
        StillSyntaxHighlightKind::Field => lsp_types::SemanticTokenType::PROPERTY,
        StillSyntaxHighlightKind::Type => lsp_types::SemanticTokenType::TYPE,
        StillSyntaxHighlightKind::Variable => lsp_types::SemanticTokenType::VARIABLE,
        StillSyntaxHighlightKind::Variant => lsp_types::SemanticTokenType::ENUM_MEMBER,
        StillSyntaxHighlightKind::DeclaredVariable => lsp_types::SemanticTokenType::FUNCTION,
        StillSyntaxHighlightKind::Comment => lsp_types::SemanticTokenType::COMMENT,
        StillSyntaxHighlightKind::Number => lsp_types::SemanticTokenType::NUMBER,
        StillSyntaxHighlightKind::String => lsp_types::SemanticTokenType::STRING,
        StillSyntaxHighlightKind::TypeVariable => lsp_types::SemanticTokenType::TYPE_PARAMETER,
    }
}

fn list_still_projects_in_directory_path(
    paths: impl Iterator<Item = std::path::PathBuf>,
) -> Vec<std::path::PathBuf> {
    let mut result: Vec<std::path::PathBuf> = Vec::new();
    for path in paths {
        list_files_passing_test_in_directory_at_path_into(&mut result, path.clone(), |path| {
            path.extension().is_some_and(|ext| ext == "still")
        });
    }
    result
}

fn list_files_passing_test_in_directory_at_path_into(
    so_far: &mut Vec<std::path::PathBuf>,
    path: std::path::PathBuf,
    should_add_file: fn(&std::path::PathBuf) -> bool,
) {
    if path.is_dir() {
        if let Ok(dir_subs) = std::fs::read_dir(&path) {
            for dir_sub in dir_subs.into_iter().filter_map(Result::ok) {
                list_files_passing_test_in_directory_at_path_into(
                    so_far,
                    dir_sub.path(),
                    should_add_file,
                );
            }
        }
    } else {
        if should_add_file(&path) {
            so_far.push(path);
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
enum StillSyntaxType {
    Variable(StillName),
    Parenthesized(Option<StillSyntaxNode<Box<StillSyntaxType>>>),
    Function {
        input: Option<StillSyntaxNode<Box<StillSyntaxType>>>,
        arrow_key_symbol_range: Option<lsp_types::Range>,
        output: Option<StillSyntaxNode<Box<StillSyntaxType>>>,
    },
    Construct {
        name: StillSyntaxNode<StillName>,
        arguments: Vec<StillSyntaxNode<StillSyntaxType>>,
    },
    Record(Vec<StillSyntaxTypeField>),
}
#[derive(Clone, Debug, PartialEq)]
struct StillSyntaxTypeField {
    name: StillSyntaxNode<StillName>,
    value: Option<StillSyntaxNode<StillSyntaxType>>,
}

#[derive(Clone, Debug, PartialEq)]
enum StillSyntaxPattern {
    Char(Option<char>),
    Int {
        value: Result<i64, Box<str>>,
    },
    String {
        content: String,
        quoting_style: StillSyntaxStringQuotingStyle,
    },
    Typed {
        type_: Option<StillSyntaxNode<StillSyntaxType>>,
        pattern: Option<StillSyntaxNode<StillSyntaxPatternUntyped>>,
    },
    Record(Vec<StillSyntaxPatternField>),
}
#[derive(Clone, Debug, PartialEq)]
struct StillSyntaxPatternField {
    name: StillSyntaxNode<StillName>,
    value: Option<StillSyntaxNode<StillSyntaxPattern>>,
}
#[derive(Clone, Debug, PartialEq)]
enum StillSyntaxPatternUntyped {
    Variable(StillName),
    Ignored,
    Variant {
        name: StillSyntaxNode<StillName>,
        value: Option<StillSyntaxNode<Box<StillSyntaxPattern>>>,
    },
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum StillSyntaxStringQuotingStyle {
    SingleQuoted,
    TripleQuoted,
}

#[derive(Clone, Debug, PartialEq)]
enum StillSyntaxLetDeclaration {
    Destructuring {
        pattern: StillSyntaxNode<StillSyntaxPattern>,
        equals_key_symbol_range: Option<lsp_types::Range>,
        expression: Option<StillSyntaxNode<Box<StillSyntaxExpression>>>,
    },
    VariableDeclaration {
        start_name: StillSyntaxNode<StillName>,
        result: Option<StillSyntaxNode<Box<StillSyntaxExpression>>>,
    },
}

#[derive(Clone, Debug, PartialEq)]
enum StillSyntaxExpression {
    VariableOrCall {
        variable: StillSyntaxNode<StillName>,
        arguments: Vec<StillSyntaxNode<StillSyntaxExpression>>,
    },
    CaseOf {
        matched: Option<StillSyntaxNode<Box<StillSyntaxExpression>>>,
        of_keyword_range: Option<lsp_types::Range>,
        cases: Vec<StillSyntaxExpressionCase>,
    },
    Char(Option<char>),
    Dec(Result<f64, Box<str>>),
    Int {
        // TODO inline
        value: Result<i64, Box<str>>,
    },
    Lambda {
        parameter: Option<StillSyntaxNode<StillSyntaxPattern>>,
        arrow_key_symbol_range: Option<lsp_types::Range>,
        result: Option<StillSyntaxNode<Box<StillSyntaxExpression>>>,
    },
    Let {
        declaration: Option<StillSyntaxNode<StillSyntaxLetDeclaration>>,
        result: Option<StillSyntaxNode<Box<StillSyntaxExpression>>>,
    },
    Vec(Vec<StillSyntaxNode<StillSyntaxExpression>>),
    Parenthesized(Option<StillSyntaxNode<Box<StillSyntaxExpression>>>),
    Typed {
        type_: Option<StillSyntaxNode<StillSyntaxType>>,
        expression: Option<StillSyntaxNode<StillSyntaxExpressionUntyped>>,
    },
    Record(Vec<StillSyntaxExpressionField>),
    RecordAccess {
        record: StillSyntaxNode<Box<StillSyntaxExpression>>,
        field: Option<StillSyntaxNode<StillName>>,
    },
    RecordUpdate {
        record: Option<StillSyntaxNode<Box<StillSyntaxExpression>>>,
        spread_key_symbol_range: lsp_types::Range,
        fields: Vec<StillSyntaxExpressionField>,
    },
    String {
        content: String,
        quoting_style: StillSyntaxStringQuotingStyle,
    },
}
#[derive(Clone, Debug, PartialEq)]
enum StillSyntaxExpressionUntyped {
    Variant {
        name: StillSyntaxNode<StillName>,
        value: Option<StillSyntaxNode<Box<StillSyntaxExpression>>>,
    },
    Other(Box<StillSyntaxExpression>),
}
#[derive(Clone, Debug, PartialEq)]
struct StillSyntaxExpressionCase {
    arrow_key_symbol_range: Option<lsp_types::Range>,
    pattern: StillSyntaxNode<StillSyntaxPattern>,
    result: Option<StillSyntaxNode<StillSyntaxExpression>>,
}
#[derive(Clone, Debug, PartialEq)]
struct StillSyntaxExpressionField {
    name: StillSyntaxNode<StillName>,
    value: Option<StillSyntaxNode<StillSyntaxExpression>>,
}

#[derive(Clone, Debug, PartialEq)]
enum StillSyntaxDeclaration {
    ChoiceType {
        name: Option<StillSyntaxNode<StillName>>,
        parameters: Vec<StillSyntaxNode<StillName>>,
        equals_key_symbol_range: Option<lsp_types::Range>,
        variant0_name: Option<StillSyntaxNode<StillName>>,
        variant0_value: Option<StillSyntaxNode<StillSyntaxType>>,
        variant1_up: Vec<StillSyntaxChoiceTypeDeclarationTailingVariant>,
    },
    TypeAlias {
        alias_keyword_range: lsp_types::Range,
        name: Option<StillSyntaxNode<StillName>>,
        parameters: Vec<StillSyntaxNode<StillName>>,
        equals_key_symbol_range: Option<lsp_types::Range>,
        type_: Option<StillSyntaxNode<StillSyntaxType>>,
    },
    Variable {
        start_name: StillSyntaxNode<StillName>,
        result: Option<StillSyntaxNode<StillSyntaxExpression>>,
    },
}

#[derive(Clone, Debug, PartialEq)]
struct StillSyntaxChoiceTypeDeclarationTailingVariant {
    or_key_symbol_range: lsp_types::Range,
    name: Option<StillSyntaxNode<StillName>>,
    value: Option<StillSyntaxNode<StillSyntaxType>>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct StillSyntaxNode<Value> {
    range: lsp_types::Range,
    value: Value,
}

fn still_syntax_node_as_ref<Value>(
    still_syntax_node: &StillSyntaxNode<Value>,
) -> StillSyntaxNode<&Value> {
    StillSyntaxNode {
        range: still_syntax_node.range,
        value: &still_syntax_node.value,
    }
}
fn still_syntax_node_as_ref_map<'a, A, B>(
    still_syntax_node: &'a StillSyntaxNode<A>,
    value_change: impl Fn(&'a A) -> B,
) -> StillSyntaxNode<B> {
    StillSyntaxNode {
        range: still_syntax_node.range,
        value: value_change(&still_syntax_node.value),
    }
}
fn still_syntax_node_map<A, B>(
    still_syntax_node: StillSyntaxNode<A>,
    value_change: impl Fn(A) -> B,
) -> StillSyntaxNode<B> {
    StillSyntaxNode {
        range: still_syntax_node.range,
        value: value_change(still_syntax_node.value),
    }
}
fn still_syntax_node_unbox<Value: ?Sized>(
    still_syntax_node_box: &StillSyntaxNode<Box<Value>>,
) -> StillSyntaxNode<&Value> {
    StillSyntaxNode {
        range: still_syntax_node_box.range,
        value: &still_syntax_node_box.value,
    }
}
fn still_syntax_node_box<Value>(
    still_syntax_node_box: StillSyntaxNode<Value>,
) -> StillSyntaxNode<Box<Value>> {
    StillSyntaxNode {
        range: still_syntax_node_box.range,
        value: Box::new(still_syntax_node_box.value),
    }
}

#[derive(Clone, Debug, PartialEq)]
struct StillSyntaxProject {
    comments: Vec<StillSyntaxNode<Box<str>>>,
    declarations: Vec<Result<StillSyntaxDocumentedDeclaration, Box<str>>>,
}

#[derive(Clone, Debug, PartialEq)]
struct StillSyntaxDocumentedDeclaration {
    documentation: Option<StillSyntaxNode<Box<str>>>,
    declaration: Option<StillSyntaxNode<StillSyntaxDeclaration>>,
}

struct RetrieveTypeError {
    range: lsp_types::Range,
    message: Box<str>,
}

fn still_syntax_pattern_type(
    pattern_node: StillSyntaxNode<&StillSyntaxPattern>,
) -> Result<StillSyntaxNode<StillSyntaxType>, Vec<RetrieveTypeError>> {
    match pattern_node.value {
        StillSyntaxPattern::Char(_) => Ok(still_syntax_node_empty(still_syntax_type_char)),
        StillSyntaxPattern::Int { .. } => Ok(still_syntax_node_empty(still_syntax_type_int)),
        StillSyntaxPattern::String { .. } => Ok(still_syntax_node_empty(still_syntax_type_str)),
        StillSyntaxPattern::Typed {
            type_: maybe_type,
            pattern: _,
        } => {
            let Some(type_node) = maybe_type else {
                return Err(vec![RetrieveTypeError {
                    range: pattern_node.range,
                    message: Box::from("type missing between :here:"),
                }]);
            };
            Ok(still_syntax_node_as_ref_map(
                type_node,
                StillSyntaxType::clone,
            ))
        }
        StillSyntaxPattern::Record(fields) => {
            let mut errors: Vec<RetrieveTypeError> = Vec::new();
            let mut field_types: Vec<StillSyntaxTypeField> = Vec::with_capacity(fields.len());
            for field in fields {
                match &field.value {
                    None => {
                        errors.push(RetrieveTypeError {
                            range: field.name.range,
                            message: Box::from(field.name.value.as_str()),
                        });
                    }
                    Some(field_value_node) => {
                        match still_syntax_pattern_type(still_syntax_node_as_ref(field_value_node))
                        {
                            Err(field_value_error) => {
                                errors.extend(field_value_error);
                            }
                            Ok(field_value_type) => {
                                if errors.is_empty() {
                                    field_types.push(StillSyntaxTypeField {
                                        name: field.name.clone(),
                                        value: Some(field_value_type),
                                    });
                                }
                            }
                        }
                    }
                }
            }
            match errors.as_slice() {
                [] => Ok(still_syntax_node_empty(StillSyntaxType::Record(
                    field_types,
                ))),
                [.., _] => Err(errors),
            }
        }
    }
}
fn still_syntax_expression_type(
    // TODO take local and project bindings
    expression_node: StillSyntaxNode<&StillSyntaxExpression>,
) -> Result<StillSyntaxNode<StillSyntaxType>, Vec<RetrieveTypeError>> {
    still_syntax_expression_type_with(std::collections::HashMap::new(), expression_node)
}
fn still_syntax_expression_type_with(
    mut bindings: std::collections::HashMap<StillName, StillSyntaxNode<StillSyntaxType>>,
    expression_node: StillSyntaxNode<&StillSyntaxExpression>,
) -> Result<StillSyntaxNode<StillSyntaxType>, Vec<RetrieveTypeError>> {
    match expression_node.value {
        StillSyntaxExpression::Typed {
            type_: maybe_type,
            expression: _,
        } => {
            let Some(type_node) = maybe_type else {
                return Err(vec![RetrieveTypeError {
                    range: expression_node.range,
                    message: Box::from("type missing between :here:"),
                }]);
            };
            Ok(still_syntax_node_as_ref_map(
                type_node,
                StillSyntaxType::clone,
            ))
        }
        StillSyntaxExpression::VariableOrCall {
            variable: variable_node,
            arguments,
        } => {
            if arguments.is_empty() {
                bindings
                    .get(variable_node.value.as_str())
                    .map(|n| still_syntax_node_as_ref_map(n, StillSyntaxType::clone))
                    .ok_or_else(|| {
                        vec![RetrieveTypeError {
                            range: expression_node.range,
                            message: Box::from("could not find this name in scope"),
                        }]
                    })
            } else {
                todo!()
            }
        }
        StillSyntaxExpression::CaseOf {
            matched: _,
            of_keyword_range: _,
            cases,
        } => match cases.as_slice() {
            [] => Err(vec![RetrieveTypeError {
                range: expression_node.range,
                message: Box::from("cases are missing"),
            }]),
            [case0, ..] => match &case0.result {
                None => Err(vec![RetrieveTypeError {
                    range: expression_node.range,
                    message: Box::from("first case result is missing"),
                }]),
                Some(case0_result_node) => still_syntax_expression_type_with(
                    bindings,
                    still_syntax_node_as_ref(case0_result_node),
                ),
            },
        },
        StillSyntaxExpression::Char(_) => Ok(still_syntax_node_empty(still_syntax_type_char)),
        StillSyntaxExpression::Dec(_) => Ok(still_syntax_node_empty(still_syntax_type_dec)),
        StillSyntaxExpression::Int { .. } => Ok(still_syntax_node_empty(still_syntax_type_int)),
        StillSyntaxExpression::Lambda {
            parameter: maybe_parameter,
            arrow_key_symbol_range: _,
            result: maybe_result,
        } => {
            let Some(parameter_node) = maybe_parameter else {
                return Err(vec![RetrieveTypeError {
                    range: expression_node.range,
                    message: Box::from("lambda parameter missing between \\here ->"),
                }]);
            };
            let Some(result_node) = maybe_result else {
                return Err(vec![RetrieveTypeError {
                    range: expression_node.range,
                    message: Box::from("lambda result missing after ->"),
                }]);
            };
            let parameter_type: StillSyntaxNode<StillSyntaxType> =
                still_syntax_pattern_type(still_syntax_node_as_ref(parameter_node))?;
            todo!("add introduced pattern bindings to bindings");
            let result_type =
                still_syntax_expression_type_with(bindings, still_syntax_node_unbox(result_node))?;
            Ok(still_syntax_node_empty(StillSyntaxType::Function {
                input: Some(still_syntax_node_map(parameter_type, Box::new)),
                arrow_key_symbol_range: None,
                output: Some(still_syntax_node_map(result_type, Box::new)),
            }))
        }
        StillSyntaxExpression::Let {
            declaration: maybe_declaration,
            result,
        } => todo!(),
        StillSyntaxExpression::Vec(elements) => match elements.as_slice() {
            [] => Err(vec![RetrieveTypeError {
                range: expression_node.range,
                message: Box::from(
                    "empty vec missing a concrete type, use for example :vec int:[]",
                ),
            }]),
            [element0_node, ..] => {
                let element_type: StillSyntaxNode<StillSyntaxType> =
                    still_syntax_expression_type_with(
                        bindings,
                        still_syntax_node_as_ref(element0_node),
                    )?;
                Ok(still_syntax_node_empty(still_syntax_type_vec(element_type)))
            }
        },
        StillSyntaxExpression::Parenthesized(None) => Err(vec![RetrieveTypeError {
            range: expression_node.range,
            message: Box::from("expression inside the parens missing between (here)"),
        }]),
        StillSyntaxExpression::Parenthesized(Some(in_parens)) => {
            still_syntax_expression_type_with(bindings, still_syntax_node_unbox(in_parens))
        }
        StillSyntaxExpression::Record(fields) => todo!(),
        StillSyntaxExpression::RecordAccess { record, field } => todo!(),
        StillSyntaxExpression::RecordUpdate {
            record: maybe_record,
            spread_key_symbol_range: _,
            fields,
        } => match maybe_record {
            None => Err(vec![RetrieveTypeError {
                range: expression_node.range,
                message: Box::from("updated record is missing"),
            }]),
            Some(record_node) => {
                still_syntax_expression_type_with(bindings, still_syntax_node_unbox(record_node))
            }
        },
        StillSyntaxExpression::String { .. } => Ok(still_syntax_node_empty(still_syntax_type_str)),
    }
}

const still_syntax_type_char: StillSyntaxType = StillSyntaxType::Construct {
    name: still_syntax_node_empty(StillName::const_new("char")),
    arguments: vec![],
};
const still_syntax_type_dec: StillSyntaxType = StillSyntaxType::Construct {
    name: still_syntax_node_empty(StillName::const_new("dec")),
    arguments: vec![],
};
const still_syntax_type_int: StillSyntaxType = StillSyntaxType::Construct {
    name: still_syntax_node_empty(StillName::const_new("int")),
    arguments: vec![],
};
const still_syntax_type_str: StillSyntaxType = StillSyntaxType::Construct {
    name: still_syntax_node_empty(StillName::const_new("str")),
    arguments: vec![],
};
fn still_syntax_type_vec(element_type: StillSyntaxNode<StillSyntaxType>) -> StillSyntaxType {
    StillSyntaxType::Construct {
        name: still_syntax_node_empty(StillName::const_new("vec")),
        arguments: vec![element_type],
    }
}
const fn still_syntax_node_empty<A>(value: A) -> StillSyntaxNode<A> {
    StillSyntaxNode {
        range: lsp_types::Range {
            start: lsp_types::Position {
                line: 0,
                character: 0,
            },
            end: lsp_types::Position {
                line: 0,
                character: 0,
            },
        },
        value,
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum LineSpan {
    Single,
    Multiple,
}
fn linebreak_indented_into(so_far: &mut String, indent: usize) {
    so_far.push('\n');
    so_far.extend(std::iter::repeat_n(' ', indent));
}
fn space_or_linebreak_indented_into(so_far: &mut String, line_span: LineSpan, indent: usize) {
    match line_span {
        LineSpan::Single => {
            so_far.push(' ');
        }
        LineSpan::Multiple => {
            linebreak_indented_into(so_far, indent);
        }
    }
}

fn still_syntax_type_to_string(
    still_syntax_type: StillSyntaxNode<&StillSyntaxType>,
    indent: usize,
    comments: &[StillSyntaxNode<Box<str>>],
) -> String {
    let mut builder: String = String::new();
    still_syntax_type_not_parenthesized_into(
        &mut builder,
        indent,
        comments, // pass from parens and slice?
        still_syntax_type,
    );
    builder
}

fn still_syntax_comments_in_range(
    comments: &[StillSyntaxNode<Box<str>>],
    range: lsp_types::Range,
) -> &[StillSyntaxNode<Box<str>>] {
    if comments.is_empty() {
        return &[];
    }
    let comments_in_range_start_index: usize = comments
        .binary_search_by(|comment_node| comment_node.range.start.cmp(&range.start))
        .unwrap_or_else(|i| i);
    let comments_in_range_end_exclusive_index: usize = comments
        .binary_search_by(|comment_node| comment_node.range.start.cmp(&range.end))
        .unwrap_or_else(|i| i);
    &comments[comments_in_range_start_index..comments_in_range_end_exclusive_index]
}
fn still_syntax_comments_from_position(
    comments: &[StillSyntaxNode<Box<str>>],
    start_position: lsp_types::Position,
) -> &[StillSyntaxNode<Box<str>>] {
    let comments_in_range_start_index: usize = comments
        .binary_search_by(|comment_node| comment_node.range.start.cmp(&start_position))
        .unwrap_or_else(|i| i);
    &comments[comments_in_range_start_index..]
}

/// same caveat as `still_syntax_comments_into` apply.
/// use in combination with `still_syntax_comments_in_range`
fn still_syntax_comments_then_linebreak_indented_into(
    so_far: &mut String,
    indent: usize,
    comments: &[StillSyntaxNode<Box<str>>],
) {
    for comment_node_in_range in comments {
        still_syntax_comment_into(so_far, &comment_node_in_range.value);
        linebreak_indented_into(so_far, indent);
    }
}
/// use in combination with `still_syntax_comments_in_range`
fn still_syntax_comments_into(
    so_far: &mut String,
    indent: usize,
    comments: &[StillSyntaxNode<Box<str>>],
) {
    let mut comments_iterator = comments.iter();
    let Some(first_comment_node) = comments_iterator.next() else {
        return;
    };
    still_syntax_comment_into(so_far, &first_comment_node.value);
    for comment_node_in_range in comments_iterator {
        linebreak_indented_into(so_far, indent);
        still_syntax_comment_into(so_far, &comment_node_in_range.value);
    }
}
fn still_syntax_comment_into(so_far: &mut String, comment: &str) {
    so_far.push('#');
    so_far.push_str(comment);
}

fn still_syntax_type_to_unparenthesized(
    still_syntax_type: StillSyntaxNode<&StillSyntaxType>,
) -> StillSyntaxNode<&StillSyntaxType> {
    match still_syntax_type.value {
        StillSyntaxType::Parenthesized(Some(in_parens)) => {
            still_syntax_type_to_unparenthesized(still_syntax_node_unbox(in_parens))
        }
        _ => still_syntax_type,
    }
}

fn next_indent(current_indent: usize) -> usize {
    (current_indent + 1).next_multiple_of(4)
}

fn still_syntax_type_not_parenthesized_into(
    so_far: &mut String,
    indent: usize,
    comments: &[StillSyntaxNode<Box<str>>],
    type_node: StillSyntaxNode<&StillSyntaxType>,
) {
    match type_node.value {
        StillSyntaxType::Construct {
            name: variable,
            arguments,
        } => {
            let line_span: LineSpan = still_syntax_range_line_span(type_node.range, comments);
            so_far.push_str(&variable.value);
            let mut previous_syntax_end: lsp_types::Position = variable.range.end;
            for argument_node in arguments {
                space_or_linebreak_indented_into(so_far, line_span, next_indent(indent));
                still_syntax_comments_then_linebreak_indented_into(
                    so_far,
                    next_indent(indent),
                    still_syntax_comments_in_range(
                        comments,
                        lsp_types::Range {
                            start: previous_syntax_end,
                            end: argument_node.range.start,
                        },
                    ),
                );
                still_syntax_type_parenthesized_if_space_separated_into(
                    so_far,
                    next_indent(indent),
                    comments,
                    argument_node.range,
                    still_syntax_type_to_unparenthesized(still_syntax_node_as_ref(argument_node)),
                );
                previous_syntax_end = argument_node.range.end;
            }
        }
        StillSyntaxType::Function {
            input: maybe_input,
            arrow_key_symbol_range: _,
            output: maybe_output,
        } => still_syntax_type_function_into(
            so_far,
            comments,
            still_syntax_range_line_span(type_node.range, comments),
            indent,
            maybe_input.as_ref().map(still_syntax_node_unbox),
            indent,
            maybe_output.as_ref().map(still_syntax_node_unbox),
        ),
        StillSyntaxType::Parenthesized(None) => {
            so_far.push('(');
            let comments_in_parens: &[StillSyntaxNode<Box<str>>] =
                still_syntax_comments_in_range(comments, type_node.range);
            if !comments_in_parens.is_empty() {
                still_syntax_comments_into(so_far, indent + 1, comments_in_parens);
                linebreak_indented_into(so_far, indent);
            }
            so_far.push(')');
        }
        StillSyntaxType::Parenthesized(Some(in_parens)) => {
            let innermost: StillSyntaxNode<&StillSyntaxType> =
                still_syntax_type_to_unparenthesized(still_syntax_node_unbox(in_parens));
            let comments_before_innermost: &[StillSyntaxNode<Box<str>>] =
                still_syntax_comments_in_range(
                    comments,
                    lsp_types::Range {
                        start: type_node.range.start,
                        end: innermost.range.start,
                    },
                );
            let comments_after_innermost: &[StillSyntaxNode<Box<str>>] =
                still_syntax_comments_in_range(
                    comments,
                    lsp_types::Range {
                        start: innermost.range.end,
                        end: type_node.range.end,
                    },
                );
            if comments_before_innermost.is_empty() && comments_after_innermost.is_empty() {
                still_syntax_type_not_parenthesized_into(so_far, indent, comments, innermost);
            } else {
                still_syntax_type_parenthesized_into(
                    so_far,
                    indent,
                    comments,
                    type_node.range,
                    innermost,
                );
            }
        }
        StillSyntaxType::Record(fields) => match fields.split_first() {
            None => {
                let comments_in_curlies: &[StillSyntaxNode<Box<str>>] =
                    still_syntax_comments_in_range(comments, type_node.range);
                if comments_in_curlies.is_empty() {
                    so_far.push_str("{}");
                } else {
                    so_far.push('{');
                    still_syntax_comments_into(so_far, indent + 1, comments);
                    linebreak_indented_into(so_far, indent);
                    so_far.push('}');
                }
            }
            Some((field0, field1_up)) => {
                let line_span: LineSpan = still_syntax_range_line_span(type_node.range, comments);
                so_far.push_str("{ ");
                still_syntax_comments_then_linebreak_indented_into(
                    so_far,
                    indent + 2,
                    still_syntax_comments_in_range(
                        comments,
                        lsp_types::Range {
                            start: type_node.range.start,
                            end: field0.name.range.start,
                        },
                    ),
                );
                let previous_syntax_end: lsp_types::Position = still_syntax_type_fields_into_string(
                    so_far, indent, comments, line_span, field0, field1_up,
                );
                space_or_linebreak_indented_into(so_far, line_span, indent);
                let comments_before_closing_curly = still_syntax_comments_in_range(
                    comments,
                    lsp_types::Range {
                        start: previous_syntax_end,
                        end: type_node.range.end,
                    },
                );
                if !comments_before_closing_curly.is_empty() {
                    linebreak_indented_into(so_far, indent);
                    still_syntax_comments_then_linebreak_indented_into(
                        so_far,
                        indent,
                        comments_before_closing_curly,
                    );
                }
                so_far.push('}');
            }
        },
        StillSyntaxType::Variable(name) => {
            so_far.push_str(name);
        }
    }
}

fn still_syntax_type_function_into<'a>(
    so_far: &mut String,
    comments: &[StillSyntaxNode<Box<str>>],
    line_span: LineSpan,
    indent_for_input: usize,
    maybe_input: Option<StillSyntaxNode<&'a StillSyntaxType>>,
    indent_after_input: usize,
    maybe_output: Option<StillSyntaxNode<&'a StillSyntaxType>>,
) {
    so_far.push('\\');
    if let Some(input_node) = maybe_input {
        still_syntax_type_not_parenthesized_into(
            so_far,
            indent_for_input + 1,
            comments,
            input_node,
        );
    }
    space_or_linebreak_indented_into(so_far, line_span, indent_after_input);
    match maybe_output {
        None => {
            so_far.push_str("-> ");
        }
        Some(output_node) => {
            so_far.push_str("->");
            space_or_linebreak_indented_into(
                so_far,
                still_syntax_range_line_span(output_node.range, comments),
                next_indent(indent_after_input),
            );
            let output_node_unparenthesized: StillSyntaxNode<&StillSyntaxType> =
                still_syntax_type_to_unparenthesized(output_node);
            match output_node_unparenthesized.value {
                StillSyntaxType::Function {
                    input: output_input,
                    arrow_key_symbol_range: _,
                    output: output_maybe_output,
                } => {
                    still_syntax_type_function_into(
                        so_far,
                        comments,
                        line_span,
                        next_indent(indent_after_input),
                        output_input.as_ref().map(still_syntax_node_unbox),
                        indent_after_input,
                        output_maybe_output.as_ref().map(still_syntax_node_unbox),
                    );
                }
                _ => {
                    still_syntax_type_not_parenthesized_into(
                        so_far,
                        next_indent(indent_after_input),
                        comments,
                        output_node_unparenthesized,
                    );
                }
            }
        }
    }
}

fn still_syntax_type_parenthesized_into(
    so_far: &mut String,
    indent: usize,

    comments: &[StillSyntaxNode<Box<str>>],
    full_range: lsp_types::Range,
    innermost: StillSyntaxNode<&StillSyntaxType>,
) {
    so_far.push('(');
    let start_so_far_length: usize = so_far.len();
    still_syntax_comments_then_linebreak_indented_into(
        so_far,
        indent + 1,
        still_syntax_comments_in_range(
            comments,
            lsp_types::Range {
                start: full_range.start,
                end: innermost.range.start,
            },
        ),
    );
    still_syntax_comments_then_linebreak_indented_into(
        so_far,
        indent + 1,
        still_syntax_comments_in_range(
            comments,
            lsp_types::Range {
                start: innermost.range.end,
                end: full_range.end,
            },
        ),
    );
    still_syntax_type_not_parenthesized_into(so_far, indent + 1, comments, innermost);
    if so_far[start_so_far_length..].contains('\n') {
        linebreak_indented_into(so_far, indent);
    }
    so_far.push(')');
}
fn still_syntax_type_parenthesized_if_space_separated_into(
    so_far: &mut String,
    indent: usize,

    comments: &[StillSyntaxNode<Box<str>>],
    full_range: lsp_types::Range,
    unparenthesized: StillSyntaxNode<&StillSyntaxType>,
) {
    let is_space_separated: bool = match unparenthesized.value {
        StillSyntaxType::Variable(_)
        | StillSyntaxType::Parenthesized(_)
        | StillSyntaxType::Record(_) => false,
        StillSyntaxType::Function { .. } => true,
        StillSyntaxType::Construct { name: _, arguments } => !arguments.is_empty(),
    };
    if is_space_separated {
        still_syntax_type_parenthesized_into(so_far, indent, comments, full_range, unparenthesized);
    } else {
        still_syntax_type_not_parenthesized_into(so_far, indent, comments, unparenthesized);
    }
}
/// returns the last syntax end position
fn still_syntax_type_fields_into_string<'a>(
    so_far: &mut String,
    indent: usize,

    comments: &[StillSyntaxNode<Box<str>>],
    line_span: LineSpan,
    field0: &'a StillSyntaxTypeField,
    field1_up: &'a [StillSyntaxTypeField],
) -> lsp_types::Position {
    so_far.push_str(&field0.name.value);
    let mut previous_syntax_end: lsp_types::Position = field0.name.range.end;
    so_far.push_str(" :");
    if let Some(field0_value_node) = &field0.value {
        let comments_before_field0_value = still_syntax_comments_in_range(
            comments,
            lsp_types::Range {
                start: field0.name.range.end,
                end: field0_value_node.range.start,
            },
        );
        space_or_linebreak_indented_into(
            so_far,
            if comments_before_field0_value.is_empty() {
                still_syntax_range_line_span(
                    lsp_types::Range {
                        start: field0.name.range.end,
                        end: field0_value_node.range.end,
                    },
                    comments,
                )
            } else {
                LineSpan::Multiple
            },
            next_indent(indent + 2),
        );
        still_syntax_comments_then_linebreak_indented_into(
            so_far,
            next_indent(indent + 2),
            comments_before_field0_value,
        );
        still_syntax_type_not_parenthesized_into(
            so_far,
            next_indent(indent + 2),
            comments,
            still_syntax_node_as_ref(field0_value_node),
        );
        previous_syntax_end = field0_value_node.range.end;
    }
    for field in field1_up {
        if line_span == LineSpan::Multiple {
            linebreak_indented_into(so_far, indent);
        }
        so_far.push_str(", ");
        still_syntax_comments_then_linebreak_indented_into(
            so_far,
            indent + 2,
            still_syntax_comments_in_range(
                comments,
                lsp_types::Range {
                    start: previous_syntax_end,
                    end: field.name.range.start,
                },
            ),
        );
        so_far.push_str(&field.name.value);
        previous_syntax_end = field.name.range.end;
        so_far.push_str(" :");
        if let Some(field_value_node) = &field.value {
            let comments_before_field_value = still_syntax_comments_in_range(
                comments,
                lsp_types::Range {
                    start: field.name.range.end,
                    end: field_value_node.range.start,
                },
            );
            space_or_linebreak_indented_into(
                so_far,
                if comments_before_field_value.is_empty() {
                    still_syntax_range_line_span(
                        lsp_types::Range {
                            start: field.name.range.end,
                            end: field_value_node.range.end,
                        },
                        comments,
                    )
                } else {
                    LineSpan::Multiple
                },
                next_indent(indent + 2),
            );
            still_syntax_comments_then_linebreak_indented_into(
                so_far,
                next_indent(indent + 2),
                comments_before_field_value,
            );
            still_syntax_type_not_parenthesized_into(
                so_far,
                next_indent(indent + 2),
                comments,
                still_syntax_node_as_ref(field_value_node),
            );
            previous_syntax_end = field_value_node.range.end;
        }
    }
    previous_syntax_end
}
// TODO add indent
fn still_syntax_pattern_into(
    so_far: &mut String,
    pattern_node: StillSyntaxNode<&StillSyntaxPattern>,
) {
    match pattern_node.value {
        StillSyntaxPattern::Char(maybe_char) => still_char_into(so_far, *maybe_char),
        StillSyntaxPattern::Int {
            value: value_or_err,
        } => {
            still_int_into(so_far, value_or_err);
        }
        StillSyntaxPattern::String {
            content,
            quoting_style,
        } => still_string_into(so_far, *quoting_style, content),
        StillSyntaxPattern::Typed {
            type_: maybe_type_node,
            pattern: maybe_pattern_node_in_typed,
        } => {
            so_far.push(':');
            if let Some(type_node) = maybe_type_node {
                still_syntax_type_not_parenthesized_into(
                    so_far,
                    1,
                    &[],
                    still_syntax_node_as_ref(type_node),
                );
            }
            so_far.push(':');
            if let Some(pattern_node_in_typed) = maybe_pattern_node_in_typed {
                match &pattern_node_in_typed.value {
                    StillSyntaxPatternUntyped::Ignored => {
                        so_far.push('_');
                    }
                    StillSyntaxPatternUntyped::Variable(name) => {
                        so_far.push_str(name);
                    }
                    StillSyntaxPatternUntyped::Variant {
                        name: variable,
                        value: maybe_value,
                    } => {
                        still_name_into(so_far, &variable.value);
                        if let Some(value_node) = maybe_value {
                            so_far.push(' ');
                            still_syntax_pattern_into(so_far, still_syntax_node_unbox(value_node));
                        }
                    }
                }
            }
        }
        StillSyntaxPattern::Record(field_names) => {
            let mut field_names_iterator = field_names.iter();
            match field_names_iterator.next() {
                None => {
                    so_far.push_str("{}");
                }
                Some(field0) => {
                    so_far.push_str("{ ");
                    so_far.push_str(&field0.name.value);
                    so_far.push(' ');
                    if let Some(field0_value) = &field0.value {
                        still_syntax_pattern_into(so_far, still_syntax_node_as_ref(field0_value));
                    }
                    for field in field_names_iterator {
                        so_far.push_str(", ");
                        so_far.push_str(&field.name.value);
                        so_far.push(' ');
                        if let Some(field_value) = &field.value {
                            still_syntax_pattern_into(
                                so_far,
                                still_syntax_node_as_ref(field_value),
                            );
                        }
                    }
                    so_far.push_str(" }");
                }
            }
        }
    }
}
/// TODO inline
fn still_name_into(so_far: &mut String, name: &StillName) {
    so_far.push_str(name);
}
fn still_char_into(so_far: &mut String, maybe_char: Option<char>) {
    match maybe_char {
        None => {
            so_far.push_str("''");
        }
        Some(char) => {
            so_far.push('\'');
            match char {
                '\'' => so_far.push_str("\\'"),
                '\\' => so_far.push_str("\\\\"),
                '\t' => so_far.push_str("\\t"),
                '\n' => so_far.push_str("\\n"),
                '\u{000D}' => so_far.push_str("\\u{000D}"),
                other_character => {
                    if still_char_needs_unicode_escaping(other_character) {
                        still_unicode_char_escape_into(so_far, other_character);
                    } else {
                        so_far.push(other_character);
                    }
                }
            }
            so_far.push('\'');
        }
    }
}
fn still_char_needs_unicode_escaping(char: char) -> bool {
    // I'm aware this isn't the exact criterion that still-format uses
    // (something something separators, private use, unassigned, ?)
    (char.len_utf16() >= 2) || char.is_control()
}
fn still_unicode_char_escape_into(so_far: &mut String, char: char) {
    for utf16_code in char.encode_utf16(&mut [0; 2]) {
        use std::fmt::Write as _;
        let _ = write!(so_far, "\\u{{{:04X}}}", utf16_code);
    }
}
fn still_int_into(so_far: &mut String, value_or_err: &Result<i64, Box<str>>) {
    match value_or_err {
        Err(value_as_string) => {
            so_far.push_str(value_as_string);
        }
        Ok(value) => {
            use std::fmt::Write as _;
            let _ = write!(so_far, "{}", value);
        }
    }
}
fn still_string_into(
    so_far: &mut String,
    quoting_style: StillSyntaxStringQuotingStyle,
    content: &str,
) {
    match quoting_style {
        StillSyntaxStringQuotingStyle::SingleQuoted => {
            so_far.push('"');
            for char in content.chars() {
                match char {
                    '\"' => so_far.push_str("\\\""),
                    '\\' => so_far.push_str("\\\\"),
                    '\t' => so_far.push_str("\\t"),
                    '\n' => so_far.push_str("\\n"),
                    '\u{000D}' => so_far.push_str("\\u{000D}"),
                    other_character => {
                        if still_char_needs_unicode_escaping(other_character) {
                            still_unicode_char_escape_into(so_far, other_character);
                        } else {
                            so_far.push(other_character);
                        }
                    }
                }
            }
            so_far.push('"');
        }
        StillSyntaxStringQuotingStyle::TripleQuoted => {
            so_far.push_str("\"\"\"");
            // because only quotes connected to the ending """ should be escaped to \"
            let mut quote_count_to_insert: usize = 0;
            'pushing_escaped_content: for char in content.chars() {
                if char == '\"' {
                    quote_count_to_insert += 1;
                    continue 'pushing_escaped_content;
                }
                so_far.extend(std::iter::repeat_n('\"', quote_count_to_insert));
                match char {
                    '\\' => so_far.push_str("\\\\"),
                    '\t' => so_far.push_str("\\t"),
                    '\r' => so_far.push('\r'),
                    '\n' => so_far.push('\n'),
                    '\"' => {
                        quote_count_to_insert += 1;
                    }
                    other_character => {
                        if still_char_needs_unicode_escaping(other_character) {
                            still_unicode_char_escape_into(so_far, other_character);
                        } else {
                            so_far.push(other_character);
                        }
                    }
                }
            }
            so_far.extend(std::iter::repeat_n("\\\"", quote_count_to_insert));
            so_far.push_str("\"\"\"");
        }
    }
}
fn still_syntax_expression_not_parenthesized_into(
    so_far: &mut String,
    indent: usize,
    comments: &[StillSyntaxNode<Box<str>>],
    expression_node: StillSyntaxNode<&StillSyntaxExpression>,
) {
    match expression_node.value {
        StillSyntaxExpression::VariableOrCall {
            variable: variable_node,
            arguments,
        } => {
            so_far.push_str(&variable_node.value);
            if let Some((argument0_node, argument1_up)) = arguments.split_first() {
                let line_span_before_argument0: LineSpan = if variable_node.range.start.line
                    == argument0_node.range.end.line
                    && still_syntax_expression_line_span(
                        comments,
                        still_syntax_node_as_ref(argument0_node),
                    ) == LineSpan::Single
                {
                    LineSpan::Single
                } else {
                    LineSpan::Multiple
                };
                let full_line_span: LineSpan = match line_span_before_argument0 {
                    LineSpan::Multiple => LineSpan::Multiple,
                    LineSpan::Single => {
                        still_syntax_expression_line_span(comments, expression_node)
                    }
                };
                space_or_linebreak_indented_into(
                    so_far,
                    line_span_before_argument0,
                    next_indent(indent),
                );
                still_syntax_expression_parenthesized_if_space_separated_into(
                    so_far,
                    next_indent(indent),
                    comments,
                    still_syntax_node_as_ref(argument0_node),
                );
                let mut previous_syntax_end: lsp_types::Position = argument0_node.range.end;
                for argument_node in argument1_up.iter().map(still_syntax_node_as_ref) {
                    space_or_linebreak_indented_into(so_far, full_line_span, next_indent(indent));
                    still_syntax_comments_then_linebreak_indented_into(
                        so_far,
                        next_indent(indent),
                        still_syntax_comments_in_range(
                            comments,
                            lsp_types::Range {
                                start: previous_syntax_end,
                                end: argument_node.range.start,
                            },
                        ),
                    );
                    still_syntax_expression_parenthesized_if_space_separated_into(
                        so_far,
                        next_indent(indent),
                        comments,
                        argument_node,
                    );
                    previous_syntax_end = argument_node.range.end;
                }
            }
        }
        StillSyntaxExpression::CaseOf {
            matched: maybe_matched,
            of_keyword_range: maybe_of_keyword_range,
            cases,
        } => {
            so_far.push_str("case");
            let previous_syntax_that_covered_comments_end: lsp_types::Position;
            match maybe_matched {
                None => match maybe_of_keyword_range {
                    None => {
                        so_far.push_str("  ");
                        previous_syntax_that_covered_comments_end = expression_node.range.start;
                    }
                    Some(of_keyword_range) => {
                        let comments_between_case_and_of_keywords: &[StillSyntaxNode<Box<str>>] =
                            still_syntax_comments_in_range(
                                comments,
                                lsp_types::Range {
                                    start: expression_node.range.start,
                                    end: of_keyword_range.end,
                                },
                            );
                        if comments_between_case_and_of_keywords.is_empty() {
                            so_far.push_str("  ");
                        } else {
                            linebreak_indented_into(so_far, next_indent(indent));
                            still_syntax_comments_into(
                                so_far,
                                next_indent(indent),
                                comments_between_case_and_of_keywords,
                            );
                            linebreak_indented_into(so_far, indent);
                        }
                        previous_syntax_that_covered_comments_end = of_keyword_range.end;
                    }
                },
                Some(matched_node) => {
                    let comments_before_matched: &[StillSyntaxNode<Box<str>>] =
                        still_syntax_comments_in_range(
                            comments,
                            lsp_types::Range {
                                start: expression_node.range.start,
                                end: matched_node.range.start,
                            },
                        );
                    let comments_before_of_keyword: &[StillSyntaxNode<Box<str>>] = if cases
                        .is_empty()
                        && let Some(of_keyword_range) = maybe_of_keyword_range
                    {
                        still_syntax_comments_in_range(
                            comments,
                            lsp_types::Range {
                                start: matched_node.range.start,
                                end: of_keyword_range.start,
                            },
                        )
                    } else {
                        &[]
                    };
                    let before_cases_line_span: LineSpan = if comments_before_matched.is_empty()
                        && comments_before_of_keyword.is_empty()
                    {
                        still_syntax_expression_line_span(
                            comments,
                            still_syntax_node_unbox(matched_node),
                        )
                    } else {
                        LineSpan::Multiple
                    };
                    space_or_linebreak_indented_into(
                        so_far,
                        before_cases_line_span,
                        next_indent(indent),
                    );
                    still_syntax_comments_then_linebreak_indented_into(
                        so_far,
                        next_indent(indent),
                        comments_before_matched,
                    );
                    still_syntax_expression_not_parenthesized_into(
                        so_far,
                        next_indent(indent),
                        comments,
                        still_syntax_node_unbox(matched_node),
                    );
                    space_or_linebreak_indented_into(so_far, before_cases_line_span, indent);
                    if let Some(of_keyword_range) = maybe_of_keyword_range
                        && !comments_before_of_keyword.is_empty()
                    {
                        linebreak_indented_into(so_far, indent);
                        still_syntax_comments_then_linebreak_indented_into(
                            so_far,
                            next_indent(indent),
                            comments_before_matched,
                        );
                        previous_syntax_that_covered_comments_end = of_keyword_range.end;
                    } else {
                        previous_syntax_that_covered_comments_end = matched_node.range.end;
                    }
                }
            }
            so_far.push_str("of");
            linebreak_indented_into(so_far, next_indent(indent));
            if let Some((case0, case1_up)) = cases.split_first() {
                let mut previous_syntax_end: lsp_types::Position = still_syntax_case_into(
                    so_far,
                    next_indent(indent),
                    comments,
                    previous_syntax_that_covered_comments_end,
                    case0,
                );
                for case in case1_up {
                    so_far.push('\n');
                    linebreak_indented_into(so_far, next_indent(indent));
                    previous_syntax_end = still_syntax_case_into(
                        so_far,
                        next_indent(indent),
                        comments,
                        previous_syntax_end,
                        case,
                    );
                }
            }
        }
        StillSyntaxExpression::Char(maybe_char) => {
            still_char_into(so_far, *maybe_char);
        }
        StillSyntaxExpression::Dec(value_or_whatever) => match value_or_whatever {
            Err(whatever) => {
                so_far.push_str(whatever);
            }
            Ok(value) => {
                use std::fmt::Write as _;
                let _ = write!(so_far, "{}", *value);
            }
        },
        StillSyntaxExpression::Int {
            value: value_or_err,
        } => {
            still_int_into(so_far, value_or_err);
        }
        StillSyntaxExpression::Lambda {
            parameter: maybe_parameter,
            arrow_key_symbol_range: maybe_arrow_key_symbol_range,
            result: maybe_result,
        } => {
            so_far.push('\\');
            let parameter_comments = still_syntax_comments_in_range(
                comments,
                lsp_types::Range {
                    start: expression_node.range.start,
                    end: if maybe_result.is_none()
                        && let Some(arrow_key_symbol_range) = maybe_arrow_key_symbol_range
                    {
                        arrow_key_symbol_range.end
                    } else {
                        maybe_parameter
                            .as_ref()
                            .map(|node| node.range.end)
                            .unwrap_or(expression_node.range.start)
                    },
                },
            );
            let mut previous_parameter_end: lsp_types::Position = expression_node.range.start;
            if let Some(parameter_node) = maybe_parameter {
                still_syntax_comments_then_linebreak_indented_into(
                    so_far,
                    indent + 1,
                    still_syntax_comments_in_range(
                        parameter_comments,
                        lsp_types::Range {
                            start: previous_parameter_end,
                            end: parameter_node.range.start,
                        },
                    ),
                );
                still_syntax_pattern_into(so_far, still_syntax_node_as_ref(parameter_node));
                let line_span: LineSpan = if parameter_comments.is_empty() {
                    LineSpan::Single
                } else {
                    LineSpan::Multiple
                };
                space_or_linebreak_indented_into(so_far, line_span, indent);
                previous_parameter_end = parameter_node.range.end;
            }
            if maybe_result.is_none()
                && let Some(arrow_key_symbol_range) = maybe_arrow_key_symbol_range
                && let comments_before_arrow_key_symbol = still_syntax_comments_in_range(
                    comments,
                    lsp_types::Range {
                        start: previous_parameter_end,
                        end: arrow_key_symbol_range.start,
                    },
                )
                && !comments_before_arrow_key_symbol.is_empty()
            {
                linebreak_indented_into(so_far, indent);
                still_syntax_comments_then_linebreak_indented_into(
                    so_far,
                    indent,
                    comments_before_arrow_key_symbol,
                );
            }
            so_far.push_str("->");
            space_or_linebreak_indented_into(
                so_far,
                still_syntax_expression_line_span(comments, expression_node),
                next_indent(indent),
            );
            if let Some(result_node) = maybe_result {
                still_syntax_comments_then_linebreak_indented_into(
                    so_far,
                    next_indent(indent),
                    still_syntax_comments_in_range(
                        comments,
                        lsp_types::Range {
                            start: previous_parameter_end,
                            end: result_node.range.start,
                        },
                    ),
                );
                still_syntax_expression_not_parenthesized_into(
                    so_far,
                    next_indent(indent),
                    comments,
                    still_syntax_node_unbox(result_node),
                );
            }
        }
        StillSyntaxExpression::Let {
            declaration: maybe_declaration,
            result: maybe_result,
        } => {
            so_far.push_str("let");
            let mut previous_declaration_end: lsp_types::Position = expression_node.range.end;
            match maybe_declaration {
                None => {
                    linebreak_indented_into(so_far, next_indent(indent));
                }
                Some(declaration_node) => {
                    linebreak_indented_into(so_far, next_indent(indent));
                    still_syntax_comments_then_linebreak_indented_into(
                        so_far,
                        next_indent(indent),
                        still_syntax_comments_in_range(
                            comments,
                            lsp_types::Range {
                                start: previous_declaration_end,
                                end: declaration_node.range.start,
                            },
                        ),
                    );
                    still_syntax_let_declaration_into(
                        so_far,
                        next_indent(indent),
                        comments,
                        still_syntax_node_as_ref(declaration_node),
                    );
                    previous_declaration_end = declaration_node.range.end;
                }
            }
            linebreak_indented_into(so_far, indent);
            if let Some(result_node) = maybe_result {
                still_syntax_comments_then_linebreak_indented_into(
                    so_far,
                    indent,
                    still_syntax_comments_in_range(
                        comments,
                        lsp_types::Range {
                            start: previous_declaration_end,
                            end: result_node.range.start,
                        },
                    ),
                );
                still_syntax_expression_not_parenthesized_into(
                    so_far,
                    indent,
                    comments,
                    still_syntax_node_unbox(result_node),
                );
            }
        }
        StillSyntaxExpression::Vec(elements) => {
            let comments: &[StillSyntaxNode<Box<str>>] =
                still_syntax_comments_in_range(comments, expression_node.range);
            match elements.split_last() {
                None => {
                    if comments.is_empty() {
                        so_far.push_str("[]");
                    } else {
                        so_far.push('[');
                        still_syntax_comments_into(so_far, indent + 1, comments);
                        linebreak_indented_into(so_far, indent);
                        so_far.push(']');
                    }
                }
                Some((last_element_node, elements_before_last)) => {
                    so_far.push_str("[ ");
                    let line_span: LineSpan =
                        still_syntax_expression_line_span(comments, expression_node);
                    let mut previous_element_end: lsp_types::Position = expression_node.range.start;
                    for element_node in elements_before_last {
                        still_syntax_comments_then_linebreak_indented_into(
                            so_far,
                            indent,
                            still_syntax_comments_in_range(
                                comments,
                                lsp_types::Range {
                                    start: previous_element_end,
                                    end: element_node.range.start,
                                },
                            ),
                        );
                        still_syntax_expression_not_parenthesized_into(
                            so_far,
                            indent + 2,
                            comments,
                            still_syntax_node_as_ref(element_node),
                        );
                        if line_span == LineSpan::Multiple {
                            linebreak_indented_into(so_far, indent);
                        }
                        so_far.push_str(", ");
                        previous_element_end = element_node.range.end;
                    }
                    still_syntax_comments_then_linebreak_indented_into(
                        so_far,
                        indent + 2,
                        still_syntax_comments_in_range(
                            comments,
                            lsp_types::Range {
                                start: previous_element_end,
                                end: last_element_node.range.start,
                            },
                        ),
                    );
                    still_syntax_expression_not_parenthesized_into(
                        so_far,
                        indent + 2,
                        comments,
                        still_syntax_node_as_ref(last_element_node),
                    );
                    space_or_linebreak_indented_into(so_far, line_span, indent);
                    let comments_after_last_element = still_syntax_comments_in_range(
                        comments,
                        lsp_types::Range {
                            start: last_element_node.range.end,
                            end: expression_node.range.end,
                        },
                    );
                    if !comments_after_last_element.is_empty() {
                        linebreak_indented_into(so_far, indent);
                        still_syntax_comments_then_linebreak_indented_into(
                            so_far,
                            indent + 2,
                            comments_after_last_element,
                        );
                    }
                    so_far.push(']');
                }
            }
        }
        StillSyntaxExpression::Parenthesized(None) => {
            so_far.push('(');
            let comments_in_parens: &[StillSyntaxNode<Box<str>>] =
                still_syntax_comments_in_range(comments, expression_node.range);
            if !comments_in_parens.is_empty() {
                still_syntax_comments_into(so_far, indent + 1, comments_in_parens);
                linebreak_indented_into(so_far, indent);
            }
            so_far.push(')');
        }
        StillSyntaxExpression::Parenthesized(Some(in_parens)) => {
            let innermost: StillSyntaxNode<&StillSyntaxExpression> =
                still_syntax_expression_to_unparenthesized(still_syntax_node_unbox(in_parens));
            let comments_before_innermost = still_syntax_comments_in_range(
                comments,
                lsp_types::Range {
                    start: expression_node.range.start,
                    end: innermost.range.start,
                },
            );
            let comments_after_innermost = still_syntax_comments_in_range(
                comments,
                lsp_types::Range {
                    start: innermost.range.end,
                    end: expression_node.range.end,
                },
            );
            if comments_before_innermost.is_empty() && comments_after_innermost.is_empty() {
                still_syntax_expression_not_parenthesized_into(so_far, indent, comments, innermost);
            } else {
                still_syntax_expression_parenthesized_into(
                    so_far,
                    indent,
                    comments,
                    expression_node.range,
                    innermost,
                );
            }
        }
        StillSyntaxExpression::Typed {
            type_: maybe_type,
            expression: maybe_expression,
        } => {
            so_far.push(':');
            if let Some(type_node) = maybe_type {
                still_syntax_type_not_parenthesized_into(
                    so_far,
                    1,
                    &[],
                    still_syntax_node_as_ref(type_node),
                );
                space_or_linebreak_indented_into(
                    so_far,
                    still_syntax_range_line_span(type_node.range, comments),
                    indent,
                );
            }
            so_far.push(':');
            if let Some(expression_node_in_typed) = maybe_expression {
                space_or_linebreak_indented_into(
                    so_far,
                    still_syntax_range_line_span(expression_node.range, comments),
                    indent,
                );
                match &expression_node_in_typed.value {
                    StillSyntaxExpressionUntyped::Variant {
                        name: name_node,
                        value: maybe_value,
                    } => {
                        so_far.push_str(&name_node.value);
                        if let Some(value_node) = maybe_value {
                            let line_span: LineSpan =
                                still_syntax_range_line_span(expression_node.range, comments);
                            space_or_linebreak_indented_into(
                                so_far,
                                line_span,
                                next_indent(indent),
                            );
                            still_syntax_expression_not_parenthesized_into(
                                so_far,
                                next_indent(indent),
                                comments,
                                still_syntax_node_unbox(value_node),
                            );
                        }
                    }
                    StillSyntaxExpressionUntyped::Other(expression_node_other_in_typed) => {
                        still_syntax_expression_not_parenthesized_into(
                            so_far,
                            indent,
                            comments,
                            StillSyntaxNode {
                                range: expression_node_in_typed.range,
                                value: expression_node_other_in_typed,
                            },
                        );
                    }
                }
            }
        }
        StillSyntaxExpression::Record(fields) => match fields.split_first() {
            None => {
                let comments_in_curlies: &[StillSyntaxNode<Box<str>>] =
                    still_syntax_comments_in_range(comments, expression_node.range);
                if comments_in_curlies.is_empty() {
                    so_far.push_str("{}");
                } else {
                    so_far.push('{');
                    still_syntax_comments_into(so_far, indent + 1, comments);
                    linebreak_indented_into(so_far, indent);
                    so_far.push('}');
                }
            }
            Some((field0, field1_up)) => {
                let line_span: LineSpan =
                    still_syntax_range_line_span(expression_node.range, comments);
                so_far.push_str("{ ");
                still_syntax_comments_then_linebreak_indented_into(
                    so_far,
                    indent + 2,
                    still_syntax_comments_in_range(
                        comments,
                        lsp_types::Range {
                            start: expression_node.range.start,
                            end: field0.name.range.start,
                        },
                    ),
                );
                let previous_syntax_end: lsp_types::Position =
                    still_syntax_expression_fields_into_string(
                        so_far, indent, comments, line_span, field0, field1_up,
                    );
                space_or_linebreak_indented_into(so_far, line_span, indent);
                let comments_before_closing_curly = still_syntax_comments_in_range(
                    comments,
                    lsp_types::Range {
                        start: previous_syntax_end,
                        end: expression_node.range.end,
                    },
                );
                if !comments_before_closing_curly.is_empty() {
                    linebreak_indented_into(so_far, indent);
                    still_syntax_comments_then_linebreak_indented_into(
                        so_far,
                        indent,
                        comments_before_closing_curly,
                    );
                }
                so_far.push('}');
            }
        },
        StillSyntaxExpression::RecordAccess {
            record,
            field: maybe_field,
        } => {
            still_syntax_expression_parenthesized_if_space_separated_into(
                so_far,
                indent,
                comments,
                still_syntax_node_unbox(record),
            );
            so_far.push('.');
            if let Some(field_name_node) = maybe_field {
                so_far.push_str(&field_name_node.value);
            }
        }
        StillSyntaxExpression::RecordUpdate {
            record: maybe_record,
            spread_key_symbol_range: _,
            fields,
        } => {
            let line_span: LineSpan = still_syntax_range_line_span(expression_node.range, comments);
            so_far.push_str("{ ..");
            let mut previous_syntax_end: lsp_types::Position = expression_node.range.start;
            if let Some(record_node) = maybe_record {
                still_syntax_expression_not_parenthesized_into(
                    so_far,
                    indent + 4,
                    comments,
                    still_syntax_node_unbox(record_node),
                );
                previous_syntax_end = record_node.range.end;
            }
            if let Some((field0, field1_up)) = fields.split_first() {
                space_or_linebreak_indented_into(so_far, line_span, indent);
                so_far.push_str(", ");
                previous_syntax_end = still_syntax_expression_fields_into_string(
                    so_far, indent, comments, line_span, field0, field1_up,
                );
            }
            space_or_linebreak_indented_into(so_far, line_span, indent);
            let comments_before_closing_curly = still_syntax_comments_in_range(
                comments,
                lsp_types::Range {
                    start: previous_syntax_end,
                    end: expression_node.range.end,
                },
            );
            if !comments_before_closing_curly.is_empty() {
                linebreak_indented_into(so_far, indent);
                still_syntax_comments_then_linebreak_indented_into(
                    so_far,
                    indent + 2,
                    comments_before_closing_curly,
                );
            }
            so_far.push('}');
        }
        StillSyntaxExpression::String {
            content,
            quoting_style,
        } => {
            still_string_into(so_far, *quoting_style, content);
        }
    }
}
/// returns the last syntax end position
fn still_syntax_case_into(
    so_far: &mut String,
    indent: usize,
    comments: &[StillSyntaxNode<Box<str>>],
    previous_syntax_end: lsp_types::Position,
    case: &StillSyntaxExpressionCase,
) -> lsp_types::Position {
    let before_case_arrow_key_symbol: lsp_types::Position = case
        .arrow_key_symbol_range
        .map(|range| range.end)
        .unwrap_or(case.pattern.range.end);
    still_syntax_comments_then_linebreak_indented_into(
        so_far,
        indent,
        still_syntax_comments_in_range(
            comments,
            lsp_types::Range {
                start: previous_syntax_end,
                end: before_case_arrow_key_symbol,
            },
        ),
    );
    still_syntax_pattern_into(so_far, still_syntax_node_as_ref(&case.pattern));
    so_far.push_str(" ->");
    linebreak_indented_into(so_far, next_indent(indent));
    if let Some(result_node) = &case.result {
        still_syntax_comments_then_linebreak_indented_into(
            so_far,
            next_indent(indent),
            still_syntax_comments_in_range(
                comments,
                lsp_types::Range {
                    start: before_case_arrow_key_symbol,
                    end: result_node.range.end,
                },
            ),
        );
        still_syntax_expression_not_parenthesized_into(
            so_far,
            next_indent(indent),
            comments,
            still_syntax_node_as_ref(result_node),
        );
        result_node.range.end
    } else {
        before_case_arrow_key_symbol
    }
}
/// returns the last syntax end position
fn still_syntax_expression_fields_into_string<'a>(
    so_far: &mut String,
    indent: usize,
    comments: &[StillSyntaxNode<Box<str>>],
    line_span: LineSpan,
    field0: &'a StillSyntaxExpressionField,
    field1_up: &'a [StillSyntaxExpressionField],
) -> lsp_types::Position {
    so_far.push_str(&field0.name.value);
    let mut previous_syntax_end: lsp_types::Position = field0.name.range.end;
    so_far.push_str(" =");
    if let Some(field0_value_node) = &field0.value {
        let comments_before_field0_value = still_syntax_comments_in_range(
            comments,
            lsp_types::Range {
                start: field0.name.range.end,
                end: field0_value_node.range.start,
            },
        );
        space_or_linebreak_indented_into(
            so_far,
            if comments_before_field0_value.is_empty() {
                still_syntax_expression_line_span(
                    comments,
                    still_syntax_node_as_ref(field0_value_node),
                )
            } else {
                LineSpan::Multiple
            },
            next_indent(indent + 2),
        );
        still_syntax_comments_then_linebreak_indented_into(
            so_far,
            next_indent(indent + 2),
            comments_before_field0_value,
        );
        still_syntax_expression_not_parenthesized_into(
            so_far,
            next_indent(indent + 2),
            comments,
            still_syntax_node_as_ref(field0_value_node),
        );
        previous_syntax_end = field0_value_node.range.end;
    }
    for field in field1_up {
        if line_span == LineSpan::Multiple {
            linebreak_indented_into(so_far, indent);
        }
        so_far.push_str(", ");
        still_syntax_comments_then_linebreak_indented_into(
            so_far,
            indent + 2,
            still_syntax_comments_in_range(
                comments,
                lsp_types::Range {
                    start: previous_syntax_end,
                    end: field.name.range.start,
                },
            ),
        );
        so_far.push_str(&field.name.value);
        previous_syntax_end = field.name.range.end;
        so_far.push_str(" =");
        if let Some(field_value_node) = &field.value {
            let comments_before_field_value = still_syntax_comments_in_range(
                comments,
                lsp_types::Range {
                    start: field.name.range.end,
                    end: field_value_node.range.start,
                },
            );
            space_or_linebreak_indented_into(
                so_far,
                if comments_before_field_value.is_empty() {
                    still_syntax_range_line_span(
                        lsp_types::Range {
                            start: field.name.range.end,
                            end: field_value_node.range.end,
                        },
                        comments,
                    )
                } else {
                    LineSpan::Multiple
                },
                next_indent(indent + 2),
            );
            still_syntax_comments_then_linebreak_indented_into(
                so_far,
                next_indent(indent + 2),
                comments_before_field_value,
            );
            still_syntax_expression_not_parenthesized_into(
                so_far,
                next_indent(indent + 2),
                comments,
                still_syntax_node_as_ref(field_value_node),
            );
            previous_syntax_end = field_value_node.range.end;
        }
    }
    previous_syntax_end
}
fn still_syntax_let_declaration_into(
    so_far: &mut String,
    indent: usize,
    comments: &[StillSyntaxNode<Box<str>>],
    let_declaration_node: StillSyntaxNode<&StillSyntaxLetDeclaration>,
) {
    match let_declaration_node.value {
        StillSyntaxLetDeclaration::Destructuring {
            pattern: pattern_node,
            equals_key_symbol_range: maybe_equals_key_symbol_range,
            expression: maybe_expression,
        } => {
            still_syntax_comments_into(
                so_far,
                indent,
                still_syntax_comments_in_range(
                    comments,
                    lsp_types::Range {
                        start: let_declaration_node.range.start,
                        end: maybe_equals_key_symbol_range
                            .map(|range| range.start)
                            .unwrap_or(pattern_node.range.end),
                    },
                ),
            );
            still_syntax_pattern_into(so_far, still_syntax_node_as_ref(pattern_node));
            so_far.push_str(" =");
            linebreak_indented_into(so_far, next_indent(indent));
            if let Some(expression_node) = maybe_expression {
                still_syntax_comments_into(
                    so_far,
                    next_indent(indent),
                    still_syntax_comments_in_range(
                        comments,
                        lsp_types::Range {
                            start: maybe_equals_key_symbol_range
                                .map(|range| range.end)
                                .unwrap_or(pattern_node.range.end),
                            end: expression_node.range.end,
                        },
                    ),
                );
            }
        }
        StillSyntaxLetDeclaration::VariableDeclaration {
            start_name: start_name_node,
            result: maybe_result,
        } => {
            still_syntax_variable_declaration_into(
                so_far,
                indent,
                comments,
                still_syntax_node_as_ref_map(start_name_node, StillName::as_str),
                maybe_result.as_ref().map(still_syntax_node_unbox),
            );
        }
    }
}
fn still_syntax_variable_declaration_into(
    so_far: &mut String,
    indent: usize,
    comments: &[StillSyntaxNode<Box<str>>],
    start_name_node: StillSyntaxNode<&str>,
    maybe_result: Option<StillSyntaxNode<&StillSyntaxExpression>>,
) {
    so_far.push_str(start_name_node.value);
    so_far.push(' ');
    if maybe_result.is_none() {
        linebreak_indented_into(so_far, indent);
        still_syntax_comments_then_linebreak_indented_into(so_far, next_indent(indent), comments);
    }
    so_far.push('=');
    linebreak_indented_into(so_far, next_indent(indent));
    if let Some(result_node) = maybe_result {
        still_syntax_comments_then_linebreak_indented_into(
            so_far,
            next_indent(indent),
            still_syntax_comments_in_range(
                comments,
                lsp_types::Range {
                    start: start_name_node.range.start,
                    end: result_node.range.start,
                },
            ),
        );
        still_syntax_expression_not_parenthesized_into(
            so_far,
            next_indent(indent),
            comments,
            result_node,
        );
    }
}
fn still_syntax_expression_to_unparenthesized(
    expression_node: StillSyntaxNode<&StillSyntaxExpression>,
) -> StillSyntaxNode<&StillSyntaxExpression> {
    match expression_node.value {
        StillSyntaxExpression::Parenthesized(Some(in_parens)) => {
            still_syntax_expression_to_unparenthesized(still_syntax_node_unbox(in_parens))
        }
        _ => expression_node,
    }
}
fn still_syntax_range_line_span(
    range: lsp_types::Range,
    comments: &[StillSyntaxNode<Box<str>>],
) -> LineSpan {
    if still_syntax_comments_in_range(comments, range).is_empty()
        && range.start.line == range.end.line
    {
        LineSpan::Single
    } else {
        LineSpan::Multiple
    }
}
/// A more accurate (but probably slower) alternative:
/// ```rust
/// let so_far_length_before = so_far.len();
/// ...into(so_far, ...);
/// if so_far[so_far_length_before..].contains('\n') {
///     so_far.insert_str(so_far_length_before, ..linebreak indented..);
/// } else {
///     so_far.insert(so_far_length_before, ' ');
/// }
/// ```
/// with a potential optimization being
fn still_syntax_expression_line_span(
    comments: &[StillSyntaxNode<Box<str>>],
    expression_node: StillSyntaxNode<&StillSyntaxExpression>,
) -> LineSpan {
    if still_syntax_comments_in_range(comments, expression_node.range).is_empty()
        && expression_node.range.start.line == expression_node.range.end.line
        && !still_syntax_expression_any_sub(expression_node, |sub_node| match sub_node.value {
            StillSyntaxExpression::CaseOf { .. } => true,
            StillSyntaxExpression::Let { .. } => true,
            StillSyntaxExpression::String {
                content,
                quoting_style,
            } => {
                *quoting_style == StillSyntaxStringQuotingStyle::TripleQuoted
                    && content.contains('\n')
            }
            StillSyntaxExpression::Int { .. }
            | StillSyntaxExpression::Dec(_)
            | StillSyntaxExpression::Char(_)
            | StillSyntaxExpression::Parenthesized(_)
            | StillSyntaxExpression::Typed { .. }
            | StillSyntaxExpression::Vec(_)
            | StillSyntaxExpression::Lambda { .. }
            | StillSyntaxExpression::Record(_)
            | StillSyntaxExpression::RecordUpdate { .. }
            | StillSyntaxExpression::RecordAccess { .. }
            | StillSyntaxExpression::VariableOrCall { .. } => false,
        })
    {
        LineSpan::Single
    } else {
        LineSpan::Multiple
    }
}
fn still_syntax_expression_parenthesized_into(
    so_far: &mut String,
    indent: usize,
    comments: &[StillSyntaxNode<Box<str>>],
    full_range: lsp_types::Range,
    innermost: StillSyntaxNode<&StillSyntaxExpression>,
) {
    so_far.push('(');
    let start_so_far_length: usize = so_far.len();
    still_syntax_comments_then_linebreak_indented_into(
        so_far,
        indent + 1,
        still_syntax_comments_in_range(
            comments,
            lsp_types::Range {
                start: full_range.start,
                end: innermost.range.start,
            },
        ),
    );
    still_syntax_comments_then_linebreak_indented_into(
        so_far,
        indent + 1,
        still_syntax_comments_in_range(
            comments,
            lsp_types::Range {
                start: innermost.range.end,
                end: full_range.end,
            },
        ),
    );
    still_syntax_expression_not_parenthesized_into(so_far, indent + 1, comments, innermost);
    if so_far[start_so_far_length..].contains('\n') {
        linebreak_indented_into(so_far, indent);
    }
    so_far.push(')');
}
fn still_syntax_expression_parenthesized_if_space_separated_into(
    so_far: &mut String,
    indent: usize,
    comments: &[StillSyntaxNode<Box<str>>],
    expression_node: StillSyntaxNode<&StillSyntaxExpression>,
) {
    let unparenthesized: StillSyntaxNode<&StillSyntaxExpression> =
        still_syntax_expression_to_unparenthesized(expression_node);
    let is_space_separated: bool = match unparenthesized.value {
        StillSyntaxExpression::Lambda { .. } => true,
        StillSyntaxExpression::Let { .. } => true,
        StillSyntaxExpression::VariableOrCall { .. } => true,
        StillSyntaxExpression::CaseOf { .. } => true,
        StillSyntaxExpression::Typed { .. } => true,
        StillSyntaxExpression::Char(_) => false,
        StillSyntaxExpression::Dec(_) => false,
        StillSyntaxExpression::Int { .. } => false,
        StillSyntaxExpression::Vec(_) => false,
        StillSyntaxExpression::Parenthesized(_) => false,
        StillSyntaxExpression::Record(_) => false,
        StillSyntaxExpression::RecordAccess { .. } => false,
        StillSyntaxExpression::RecordUpdate { .. } => false,
        StillSyntaxExpression::String { .. } => false,
    };
    if is_space_separated {
        still_syntax_expression_parenthesized_into(
            so_far,
            indent,
            comments,
            expression_node.range,
            unparenthesized,
        );
    } else {
        still_syntax_expression_not_parenthesized_into(so_far, indent, comments, expression_node);
    }
}
fn still_syntax_expression_any_sub(
    expression_node: StillSyntaxNode<&StillSyntaxExpression>,
    is_needle: impl Fn(StillSyntaxNode<&StillSyntaxExpression>) -> bool + Copy,
) -> bool {
    if is_needle(expression_node) {
        return true;
    }
    match expression_node.value {
        StillSyntaxExpression::VariableOrCall {
            variable: _,
            arguments,
        } => arguments.iter().any(|argument_node| {
            still_syntax_expression_any_sub(still_syntax_node_as_ref(argument_node), is_needle)
        }),
        StillSyntaxExpression::CaseOf {
            matched: maybe_matched,
            of_keyword_range: _,
            cases,
        } => {
            maybe_matched.as_ref().is_some_and(|matched_node| {
                still_syntax_expression_any_sub(still_syntax_node_unbox(matched_node), is_needle)
            }) || cases
                .iter()
                .filter_map(|case| case.result.as_ref())
                .any(|case_result_node| {
                    still_syntax_expression_any_sub(
                        still_syntax_node_as_ref(case_result_node),
                        is_needle,
                    )
                })
        }
        StillSyntaxExpression::Char(_) => false,
        StillSyntaxExpression::Dec(_) => false,
        StillSyntaxExpression::Int { .. } => false,
        StillSyntaxExpression::Lambda {
            parameter: _,
            arrow_key_symbol_range: _,
            result: maybe_result,
        } => maybe_result.as_ref().is_some_and(|result_node| {
            still_syntax_expression_any_sub(still_syntax_node_unbox(result_node), is_needle)
        }),
        StillSyntaxExpression::Let {
            declaration: maybe_declaration,
            result: maybe_result,
        } => {
            maybe_result.as_ref().is_some_and(|result_node| {
                still_syntax_expression_any_sub(still_syntax_node_unbox(result_node), is_needle)
            }) || maybe_declaration
                .as_ref()
                .and_then(|declaration_node| match &declaration_node.value {
                    StillSyntaxLetDeclaration::Destructuring {
                        pattern: _,
                        equals_key_symbol_range: _,
                        expression,
                    } => expression.as_ref(),
                    StillSyntaxLetDeclaration::VariableDeclaration {
                        start_name: _,
                        result,
                    } => result.as_ref(),
                })
                .is_some_and(|declaration_expression_node| {
                    still_syntax_expression_any_sub(
                        still_syntax_node_unbox(declaration_expression_node),
                        is_needle,
                    )
                })
        }
        StillSyntaxExpression::Vec(elements) => elements.iter().any(|element_node| {
            still_syntax_expression_any_sub(still_syntax_node_as_ref(element_node), is_needle)
        }),
        StillSyntaxExpression::Parenthesized(None) => false,
        StillSyntaxExpression::Parenthesized(Some(in_parens)) => {
            still_syntax_expression_any_sub(still_syntax_node_unbox(in_parens), is_needle)
        }
        StillSyntaxExpression::Typed {
            type_: _,
            expression: maybe_expression,
        } => maybe_expression
            .as_ref()
            .is_some_and(
                |expression_node_in_typed| match &expression_node_in_typed.value {
                    StillSyntaxExpressionUntyped::Variant {
                        name: _,
                        value: maybe_value,
                    } => maybe_value.as_ref().is_some_and(|argument_node| {
                        still_syntax_expression_any_sub(
                            still_syntax_node_unbox(argument_node),
                            is_needle,
                        )
                    }),
                    StillSyntaxExpressionUntyped::Other(other_expression_in_typed) => {
                        still_syntax_expression_any_sub(
                            StillSyntaxNode {
                                range: expression_node_in_typed.range,
                                value: other_expression_in_typed,
                            },
                            is_needle,
                        )
                    }
                },
            ),
        StillSyntaxExpression::Record(fields) => fields
            .iter()
            .filter_map(|field| field.value.as_ref())
            .any(|field_value_node| {
                still_syntax_expression_any_sub(
                    still_syntax_node_as_ref(field_value_node),
                    is_needle,
                )
            }),
        StillSyntaxExpression::RecordAccess { record, field: _ } => {
            still_syntax_expression_any_sub(still_syntax_node_unbox(record), is_needle)
        }
        StillSyntaxExpression::RecordUpdate {
            record: _,
            spread_key_symbol_range: _,
            fields,
        } => fields
            .iter()
            .filter_map(|field| field.value.as_ref())
            .any(|field_value_node| {
                still_syntax_expression_any_sub(
                    still_syntax_node_as_ref(field_value_node),
                    is_needle,
                )
            }),
        StillSyntaxExpression::String { .. } => false,
    }
}

fn still_syntax_project_format(project_state: &ProjectState) -> String {
    let still_syntax_project: &StillSyntaxProject = &project_state.syntax;
    let mut builder: String = String::with_capacity(project_state.source.len());
    let mut previous_syntax_end: lsp_types::Position = lsp_types::Position {
        line: 0,
        character: 0,
    };
    builder.push('\n');
    for documented_declaration_or_err in &still_syntax_project.declarations {
        match documented_declaration_or_err {
            Err(whatever) => {
                builder.push_str(whatever);
            }
            Ok(documented_declaration) => {
                builder.push_str("\n\n");
                if let Some(project_documentation_node) = &documented_declaration.documentation {
                    still_syntax_project_level_comments(
                        &mut builder,
                        still_syntax_comments_in_range(
                            &still_syntax_project.comments,
                            lsp_types::Range {
                                start: previous_syntax_end,
                                end: project_documentation_node.range.start,
                            },
                        ),
                    );
                    still_syntax_documentation_comment_then_linebreak_into(
                        &mut builder,
                        &project_documentation_node.value,
                    );
                    previous_syntax_end = project_documentation_node.range.end;
                }
                if let Some(declaration_node) = &documented_declaration.declaration {
                    still_syntax_project_level_comments(
                        &mut builder,
                        still_syntax_comments_in_range(
                            &still_syntax_project.comments,
                            lsp_types::Range {
                                start: previous_syntax_end,
                                end: declaration_node.range.start,
                            },
                        ),
                    );
                    still_syntax_declaration_into(
                        &mut builder,
                        &still_syntax_project.comments,
                        still_syntax_node_as_ref(declaration_node),
                    );
                    previous_syntax_end = declaration_node.range.end;
                    builder.push('\n');
                }
            }
        }
    }
    let comments_after_declarations: &[StillSyntaxNode<Box<str>>] =
        still_syntax_comments_from_position(&still_syntax_project.comments, previous_syntax_end);
    if !comments_after_declarations.is_empty() {
        builder.push_str("\n\n\n");
        still_syntax_comments_then_linebreak_indented_into(
            &mut builder,
            0,
            comments_after_declarations,
        );
    }
    builder
}
fn still_syntax_project_level_comments(
    so_far: &mut String,
    comments: &[StillSyntaxNode<Box<str>>],
) {
    if !comments.is_empty() {
        so_far.push('\n');
        still_syntax_comments_then_linebreak_indented_into(so_far, 0, comments);
        so_far.push_str("\n\n");
    }
}
fn still_syntax_documentation_comment_then_linebreak_into(so_far: &mut String, content: &str) {
    so_far.push_str("(#");
    so_far.push_str(content);
    so_far.push_str("#)\n");
}

fn still_syntax_declaration_into(
    so_far: &mut String,
    comments: &[StillSyntaxNode<Box<str>>],
    declaration_node: StillSyntaxNode<&StillSyntaxDeclaration>,
) {
    match declaration_node.value {
        StillSyntaxDeclaration::ChoiceType {
            name: maybe_name,
            parameters,
            equals_key_symbol_range: _,
            variant0_name: maybe_variant0_name,
            variant0_value: variant0_maybe_value,
            variant1_up,
        } => {
            still_syntax_choice_type_declaration_into(
                so_far,
                comments,
                declaration_node.range,
                maybe_name
                    .as_ref()
                    .map(|n| still_syntax_node_as_ref_map(n, StillName::as_str)),
                parameters,
                maybe_variant0_name
                    .as_ref()
                    .map(|n| still_syntax_node_as_ref_map(n, StillName::as_str)),
                variant0_maybe_value.as_ref().map(still_syntax_node_as_ref),
                variant1_up,
            );
        }
        StillSyntaxDeclaration::TypeAlias {
            alias_keyword_range: _,
            name: maybe_name,
            parameters,
            equals_key_symbol_range: _,
            type_: maybe_type,
        } => {
            still_syntax_type_alias_declaration_into(
                so_far,
                comments,
                declaration_node.range,
                maybe_name
                    .as_ref()
                    .map(|n| still_syntax_node_as_ref_map(n, StillName::as_str)),
                parameters,
                maybe_type.as_ref().map(still_syntax_node_as_ref),
            );
        }
        StillSyntaxDeclaration::Variable {
            start_name: start_name_node,
            result: maybe_result,
        } => {
            still_syntax_variable_declaration_into(
                so_far,
                0,
                comments,
                still_syntax_node_as_ref_map(start_name_node, StillName::as_str),
                maybe_result.as_ref().map(still_syntax_node_as_ref),
            );
        }
    }
}

fn still_syntax_type_alias_declaration_into(
    so_far: &mut String,
    comments: &[StillSyntaxNode<Box<str>>],
    declaration_range: lsp_types::Range,
    maybe_name: Option<StillSyntaxNode<&str>>,
    parameters: &[StillSyntaxNode<StillName>],
    maybe_type: Option<StillSyntaxNode<&StillSyntaxType>>,
) {
    let mut previous_syntax_end: lsp_types::Position = declaration_range.start;
    so_far.push_str("type alias ");
    if let Some(name_node) = maybe_name {
        still_syntax_comments_then_linebreak_indented_into(
            so_far,
            11,
            still_syntax_comments_in_range(
                comments,
                lsp_types::Range {
                    start: declaration_range.start,
                    end: name_node.range.start,
                },
            ),
        );
        so_far.push_str(name_node.value);
        previous_syntax_end = name_node.range.end;
    }
    let comments_before_and_between_parameters = match parameters.last() {
        None => &[],
        Some(last_parameter) => still_syntax_comments_in_range(
            comments,
            lsp_types::Range {
                start: previous_syntax_end,
                end: last_parameter.range.end,
            },
        ),
    };
    for parameter_node in parameters {
        if comments_before_and_between_parameters.is_empty() {
            so_far.push(' ');
        } else {
            linebreak_indented_into(so_far, 12);
            still_syntax_comments_then_linebreak_indented_into(
                so_far,
                12,
                still_syntax_comments_in_range(
                    comments,
                    lsp_types::Range {
                        start: previous_syntax_end,
                        end: parameter_node.range.start,
                    },
                ),
            );
        }
        so_far.push_str(&parameter_node.value);
        previous_syntax_end = parameter_node.range.end;
    }
    if let Some(type_node) = maybe_type {
        space_or_linebreak_indented_into(
            so_far,
            if comments_before_and_between_parameters.is_empty() {
                LineSpan::Single
            } else {
                LineSpan::Multiple
            },
            4,
        );
        so_far.push('=');
        linebreak_indented_into(so_far, 4);
        still_syntax_comments_then_linebreak_indented_into(
            so_far,
            4,
            still_syntax_comments_in_range(
                comments,
                lsp_types::Range {
                    start: previous_syntax_end,
                    end: type_node.range.start,
                },
            ),
        );
        still_syntax_type_not_parenthesized_into(so_far, 4, comments, type_node);
    }
}
fn still_syntax_choice_type_declaration_into<'a>(
    so_far: &mut String,
    comments: &[StillSyntaxNode<Box<str>>],
    declaration_range: lsp_types::Range,
    maybe_name: Option<StillSyntaxNode<&str>>,
    parameters: &[StillSyntaxNode<StillName>],
    maybe_variant0_name: Option<StillSyntaxNode<&str>>,
    variant0_maybe_value: Option<StillSyntaxNode<&'a StillSyntaxType>>,
    variant1_up: &'a [StillSyntaxChoiceTypeDeclarationTailingVariant],
) {
    let mut previous_syntax_end: lsp_types::Position = declaration_range.start;
    so_far.push_str("type ");
    if let Some(name_node) = maybe_name {
        still_syntax_comments_then_linebreak_indented_into(
            so_far,
            5,
            still_syntax_comments_in_range(
                comments,
                lsp_types::Range {
                    start: declaration_range.start,
                    end: name_node.range.start,
                },
            ),
        );
        so_far.push_str(name_node.value);
        previous_syntax_end = name_node.range.end;
    }
    let comments_before_and_between_parameters = match parameters.last() {
        None => &[],
        Some(last_parameter) => still_syntax_comments_in_range(
            comments,
            lsp_types::Range {
                start: previous_syntax_end,
                end: last_parameter.range.end,
            },
        ),
    };
    for parameter_node in parameters {
        if comments_before_and_between_parameters.is_empty() {
            so_far.push(' ');
        } else {
            linebreak_indented_into(so_far, 8);
            still_syntax_comments_then_linebreak_indented_into(
                so_far,
                8,
                still_syntax_comments_in_range(
                    comments,
                    lsp_types::Range {
                        start: previous_syntax_end,
                        end: parameter_node.range.start,
                    },
                ),
            );
        }
        so_far.push_str(&parameter_node.value);
        previous_syntax_end = parameter_node.range.end;
    }
    linebreak_indented_into(so_far, 4);
    so_far.push_str("= ");
    previous_syntax_end = still_syntax_choice_type_declaration_variant_into(
        so_far,
        comments,
        previous_syntax_end,
        maybe_variant0_name,
        variant0_maybe_value,
    );
    for variant in variant1_up {
        linebreak_indented_into(so_far, 4);
        so_far.push_str("| ");
        previous_syntax_end = still_syntax_choice_type_declaration_variant_into(
            so_far,
            comments,
            previous_syntax_end,
            variant
                .name
                .as_ref()
                .map(|n| still_syntax_node_as_ref_map(n, StillName::as_str)),
            variant.value.as_ref().map(still_syntax_node_as_ref),
        );
    }
}
fn still_syntax_choice_type_declaration_variant_into(
    so_far: &mut String,
    comments: &[StillSyntaxNode<Box<str>>],
    mut previous_syntax_end: lsp_types::Position,
    maybe_variant_name: Option<StillSyntaxNode<&str>>,
    variant_maybe_value: Option<StillSyntaxNode<&StillSyntaxType>>,
) -> lsp_types::Position {
    if let Some(variant_name_node) = maybe_variant_name {
        still_syntax_comments_then_linebreak_indented_into(
            so_far,
            6,
            still_syntax_comments_in_range(
                comments,
                lsp_types::Range {
                    start: previous_syntax_end,
                    end: variant_name_node.range.start,
                },
            ),
        );
        so_far.push_str(variant_name_node.value);
        previous_syntax_end = variant_name_node.range.end;
    }
    let Some(variant_last_value_node) = variant_maybe_value else {
        return previous_syntax_end;
    };
    let line_span: LineSpan = still_syntax_range_line_span(
        lsp_types::Range {
            start: previous_syntax_end,
            end: variant_last_value_node.range.end,
        },
        comments,
    );
    if let Some(value_node) = variant_maybe_value {
        space_or_linebreak_indented_into(so_far, line_span, 8);
        still_syntax_comments_then_linebreak_indented_into(
            so_far,
            8,
            still_syntax_comments_in_range(
                comments,
                lsp_types::Range {
                    start: previous_syntax_end,
                    end: value_node.range.start,
                },
            ),
        );
        still_syntax_type_parenthesized_if_space_separated_into(
            so_far,
            8,
            comments,
            value_node.range,
            still_syntax_type_to_unparenthesized(value_node),
        );
        previous_syntax_end = value_node.range.end;
    }
    previous_syntax_end
}

// //
#[derive(Clone, Debug)]
enum StillSyntaxSymbol<'a> {
    // includes variant
    ProjectMemberDeclarationName {
        name: &'a str,
        documentation: Option<&'a str>,
        declaration: StillSyntaxNode<&'a StillSyntaxDeclaration>,
    },
    LetDeclarationName {
        name: &'a str,
        start_name_range: lsp_types::Range,
        type_type: Option<StillSyntaxNode<StillSyntaxType>>,
        scope_expression: StillSyntaxNode<&'a StillSyntaxExpression>,
    },
    VariableOrVariant {
        name: &'a str,
        // consider wrapping in Option
        local_bindings: StillLocalBindings<'a>,
    },
    Type {
        // TODO without single-field record
        name: &'a str,
    },
    TypeVariable {
        scope_declaration: &'a StillSyntaxDeclaration,
        name: &'a str,
    },
}
type StillLocalBindings<'a> = Vec<(
    StillSyntaxNode<&'a StillSyntaxExpression>,
    Vec<StillLocalBinding<'a>>,
)>;
fn find_local_binding_scope_expression<'a>(
    local_bindings: &StillLocalBindings<'a>,
    to_find: &str,
) -> Option<(
    LocalBindingOrigin,
    StillSyntaxNode<&'a StillSyntaxExpression>,
)> {
    local_bindings
        .iter()
        .find_map(|(scope_expression, local_bindings)| {
            local_bindings.iter().find_map(|local_binding| {
                if local_binding.name == to_find {
                    Some((local_binding.origin.clone(), *scope_expression))
                } else {
                    None
                }
            })
        })
}

fn still_syntax_project_find_symbol_at_position<'a>(
    still_syntax_project: &'a StillSyntaxProject,
    position: lsp_types::Position,
) -> Option<StillSyntaxNode<StillSyntaxSymbol<'a>>> {
    still_syntax_project
        .declarations
        .iter()
        .filter_map(|declaration_or_err| declaration_or_err.as_ref().ok())
        .find_map(|documented_declaration| {
            let declaration_node = documented_declaration.declaration.as_ref()?;
            still_syntax_declaration_find_variable_at_position(
                still_syntax_node_as_ref(declaration_node),
                documented_declaration
                    .documentation
                    .as_ref()
                    .map(|node| node.value.as_ref()),
                position,
            )
        })
}

fn still_syntax_declaration_find_variable_at_position<'a>(
    still_syntax_declaration_node: StillSyntaxNode<&'a StillSyntaxDeclaration>,
    maybe_documentation: Option<&'a str>,
    position: lsp_types::Position,
) -> Option<StillSyntaxNode<StillSyntaxSymbol<'a>>> {
    if !lsp_range_includes_position(still_syntax_declaration_node.range, position) {
        None
    } else {
        match still_syntax_declaration_node.value {
            StillSyntaxDeclaration::ChoiceType {
                name: maybe_name,
                parameters,
                equals_key_symbol_range: _,
                variant0_name: maybe_variant0_name,
                variant0_value: variant0_maybe_value,
                variant1_up,
            } => {
                if let Some(name_node) = maybe_name
                    && lsp_range_includes_position(name_node.range, position)
                {
                    Some(StillSyntaxNode {
                        value: StillSyntaxSymbol::ProjectMemberDeclarationName {
                            name: &name_node.value,
                            declaration: still_syntax_declaration_node,
                            documentation: maybe_documentation,
                        },
                        range: name_node.range,
                    })
                } else if let Some(variant0_name_node) = maybe_variant0_name
                    && lsp_range_includes_position(variant0_name_node.range, position)
                {
                    Some(StillSyntaxNode {
                        value: StillSyntaxSymbol::ProjectMemberDeclarationName {
                            name: &variant0_name_node.value,
                            declaration: still_syntax_declaration_node,
                            documentation: maybe_documentation,
                        },
                        range: variant0_name_node.range,
                    })
                } else {
                    parameters
                        .iter()
                        .find_map(|parameter_node| {
                            if lsp_range_includes_position(parameter_node.range, position) {
                                Some(StillSyntaxNode {
                                    value: StillSyntaxSymbol::TypeVariable {
                                        scope_declaration: still_syntax_declaration_node.value,
                                        name: &parameter_node.value,
                                    },
                                    range: parameter_node.range,
                                })
                            } else {
                                None
                            }
                        })
                        .or_else(|| {
                            variant0_maybe_value.iter().find_map(|variant_value| {
                                still_syntax_type_find_variable_at_position(
                                    still_syntax_declaration_node.value,
                                    still_syntax_node_as_ref(variant_value),
                                    position,
                                )
                            })
                        })
                        .or_else(|| {
                            variant1_up.iter().find_map(|variant| {
                                if let Some(variant_name_node) = &variant.name
                                    && lsp_range_includes_position(
                                        variant_name_node.range,
                                        position,
                                    )
                                {
                                    Some(StillSyntaxNode {
                                        value: StillSyntaxSymbol::ProjectMemberDeclarationName {
                                            name: &variant_name_node.value,
                                            declaration: still_syntax_declaration_node,
                                            documentation: maybe_documentation,
                                        },
                                        range: variant_name_node.range,
                                    })
                                } else {
                                    variant.value.iter().find_map(|variant_value| {
                                        still_syntax_type_find_variable_at_position(
                                            still_syntax_declaration_node.value,
                                            still_syntax_node_as_ref(variant_value),
                                            position,
                                        )
                                    })
                                }
                            })
                        })
                }
            }
            StillSyntaxDeclaration::TypeAlias {
                alias_keyword_range: _,
                name: maybe_name,
                parameters,
                equals_key_symbol_range: _,
                type_: maybe_type,
            } => {
                if let Some(name_node) = maybe_name
                    && lsp_range_includes_position(name_node.range, position)
                {
                    Some(StillSyntaxNode {
                        value: StillSyntaxSymbol::ProjectMemberDeclarationName {
                            name: &name_node.value,
                            declaration: still_syntax_declaration_node,
                            documentation: maybe_documentation,
                        },
                        range: name_node.range,
                    })
                } else {
                    parameters
                        .iter()
                        .find_map(|parameter_node| {
                            if lsp_range_includes_position(parameter_node.range, position) {
                                Some(StillSyntaxNode {
                                    value: StillSyntaxSymbol::TypeVariable {
                                        scope_declaration: still_syntax_declaration_node.value,
                                        name: &parameter_node.value,
                                    },
                                    range: parameter_node.range,
                                })
                            } else {
                                None
                            }
                        })
                        .or_else(|| {
                            maybe_type.as_ref().and_then(|type_node| {
                                still_syntax_type_find_variable_at_position(
                                    still_syntax_declaration_node.value,
                                    still_syntax_node_as_ref(type_node),
                                    position,
                                )
                            })
                        })
                }
            }
            StillSyntaxDeclaration::Variable {
                start_name: start_name_node,
                result: maybe_result,
            } => {
                if lsp_range_includes_position(start_name_node.range, position) {
                    Some(StillSyntaxNode {
                        value: StillSyntaxSymbol::ProjectMemberDeclarationName {
                            name: &start_name_node.value,
                            declaration: still_syntax_declaration_node,
                            documentation: maybe_documentation,
                        },
                        range: start_name_node.range,
                    })
                } else {
                    maybe_result.as_ref().and_then(|result_node| {
                        still_syntax_expression_find_variable_at_position(
                            vec![(still_syntax_node_as_ref(result_node), Vec::new())],
                            still_syntax_declaration_node.value,
                            still_syntax_node_as_ref(result_node),
                            position,
                        )
                        .break_value()
                    })
                }
            }
        }
    }
}

fn still_syntax_pattern_find_variable_at_position<'a>(
    scope_declaration: &'a StillSyntaxDeclaration,
    still_syntax_pattern_node: StillSyntaxNode<&'a StillSyntaxPattern>,
    position: lsp_types::Position,
) -> Option<StillSyntaxNode<StillSyntaxSymbol<'a>>> {
    match still_syntax_pattern_node.value {
        StillSyntaxPattern::Char(_) => None,
        StillSyntaxPattern::Int { .. } => None,
        StillSyntaxPattern::Typed {
            type_: maybe_type_node,
            pattern: maybe_pattern_node_in_typed,
        } => maybe_type_node
            .as_ref()
            .and_then(|type_node| {
                still_syntax_type_find_variable_at_position(
                    scope_declaration,
                    still_syntax_node_as_ref(type_node),
                    position,
                )
            })
            .or_else(|| {
                let pattern_node_in_typed = maybe_pattern_node_in_typed.as_ref()?;
                match &pattern_node_in_typed.value {
                    StillSyntaxPatternUntyped::Ignored => None,
                    StillSyntaxPatternUntyped::Variable(_) => None,
                    StillSyntaxPatternUntyped::Variant {
                        name: variable,
                        value: maybe_value,
                    } => {
                        if lsp_range_includes_position(variable.range, position) {
                            Some(StillSyntaxNode {
                                value: StillSyntaxSymbol::VariableOrVariant {
                                    name: &variable.value,
                                    local_bindings: vec![],
                                },
                                range: variable.range,
                            })
                        } else {
                            maybe_value.as_ref().and_then(|value| {
                                still_syntax_pattern_find_variable_at_position(
                                    scope_declaration,
                                    still_syntax_node_unbox(value),
                                    position,
                                )
                            })
                        }
                    }
                }
            }),
        StillSyntaxPattern::Record(_) => None,
        StillSyntaxPattern::String { .. } => None,
    }
}

fn still_syntax_type_find_variable_at_position<'a>(
    scope_declaration: &'a StillSyntaxDeclaration,
    still_syntax_type_node: StillSyntaxNode<&'a StillSyntaxType>,
    position: lsp_types::Position,
) -> Option<StillSyntaxNode<StillSyntaxSymbol<'a>>> {
    if !lsp_range_includes_position(still_syntax_type_node.range, position) {
        None
    } else {
        match still_syntax_type_node.value {
            StillSyntaxType::Construct {
                name: variable,
                arguments,
            } => {
                if lsp_range_includes_position(variable.range, position) {
                    Some(StillSyntaxNode {
                        value: StillSyntaxSymbol::Type {
                            name: &variable.value,
                        },
                        range: variable.range,
                    })
                } else {
                    arguments.iter().find_map(|argument| {
                        still_syntax_type_find_variable_at_position(
                            scope_declaration,
                            still_syntax_node_as_ref(argument),
                            position,
                        )
                    })
                }
            }
            StillSyntaxType::Function {
                input: maybe_input,
                arrow_key_symbol_range: _,
                output: maybe_output,
            } => maybe_input
                .as_ref()
                .and_then(|input_node| {
                    still_syntax_type_find_variable_at_position(
                        scope_declaration,
                        still_syntax_node_unbox(input_node),
                        position,
                    )
                })
                .or_else(|| {
                    maybe_output.as_ref().and_then(|output_node| {
                        still_syntax_type_find_variable_at_position(
                            scope_declaration,
                            still_syntax_node_unbox(output_node),
                            position,
                        )
                    })
                }),
            StillSyntaxType::Parenthesized(None) => None,
            StillSyntaxType::Parenthesized(Some(in_parens)) => {
                still_syntax_type_find_variable_at_position(
                    scope_declaration,
                    still_syntax_node_unbox(in_parens),
                    position,
                )
            }
            StillSyntaxType::Record(fields) => fields.iter().find_map(|field| {
                field.value.as_ref().and_then(|field_value_node| {
                    still_syntax_type_find_variable_at_position(
                        scope_declaration,
                        still_syntax_node_as_ref(field_value_node),
                        position,
                    )
                })
            }),
            StillSyntaxType::Variable(type_variable_value) => Some(StillSyntaxNode {
                range: still_syntax_type_node.range,
                value: StillSyntaxSymbol::TypeVariable {
                    scope_declaration: scope_declaration,
                    name: type_variable_value,
                },
            }),
        }
    }
}

#[derive(Clone, Debug)]
enum LocalBindingOrigin {
    // consider separately tracking parameter names (let or otherwise), including their origin declaration name and annotation type when available
    PatternVariable(lsp_types::Range),
    LetDeclaredVariable {
        type_: Option<StillSyntaxNode<StillSyntaxType>>,
        name_range: lsp_types::Range,
    },
}
#[derive(Clone, Debug)]
struct StillLocalBinding<'a> {
    name: &'a str,
    origin: LocalBindingOrigin,
}

fn on_some_break<A>(maybe: Option<A>) -> std::ops::ControlFlow<A, ()> {
    match maybe {
        None => std::ops::ControlFlow::Continue(()),
        Some(value) => std::ops::ControlFlow::Break(value),
    }
}

fn still_syntax_expression_find_variable_at_position<'a>(
    mut local_bindings: StillLocalBindings<'a>,
    scope_declaration: &'a StillSyntaxDeclaration,
    still_syntax_expression_node: StillSyntaxNode<&'a StillSyntaxExpression>,
    position: lsp_types::Position,
) -> std::ops::ControlFlow<StillSyntaxNode<StillSyntaxSymbol<'a>>, StillLocalBindings<'a>> {
    if !lsp_range_includes_position(still_syntax_expression_node.range, position) {
        return std::ops::ControlFlow::Continue(local_bindings);
    }
    match still_syntax_expression_node.value {
        StillSyntaxExpression::VariableOrCall {
            variable: variable_node,
            arguments,
        } => {
            if lsp_range_includes_position(variable_node.range, position) {
                return std::ops::ControlFlow::Break(StillSyntaxNode {
                    value: StillSyntaxSymbol::VariableOrVariant {
                        name: &variable_node.value,
                        local_bindings: local_bindings,
                    },
                    range: still_syntax_expression_node.range,
                });
            }
            arguments
                .iter()
                .try_fold(local_bindings, |local_bindings, argument| {
                    still_syntax_expression_find_variable_at_position(
                        local_bindings,
                        scope_declaration,
                        still_syntax_node_as_ref(argument),
                        position,
                    )
                })
        }
        StillSyntaxExpression::CaseOf {
            matched: maybe_matched,
            of_keyword_range: _,
            cases,
        } => {
            if let Some(matched_node) = maybe_matched {
                local_bindings = still_syntax_expression_find_variable_at_position(
                    local_bindings,
                    scope_declaration,
                    still_syntax_node_unbox(matched_node),
                    position,
                )?;
            }
            cases
                .iter()
                .try_fold(local_bindings, |mut local_bindings, case| {
                    if let Some(found_symbol) = still_syntax_pattern_find_variable_at_position(
                        scope_declaration,
                        still_syntax_node_as_ref(&case.pattern),
                        position,
                    ) {
                        return std::ops::ControlFlow::Break(found_symbol);
                    }
                    if let Some(case_result_node) = &case.result
                    && // we need to check that the position is actually in that case before committing to mutating local bindings
                    lsp_range_includes_position(case_result_node.range, position)
                    {
                        let mut introduced_bindings: Vec<StillLocalBinding> = Vec::new();
                        still_syntax_pattern_bindings_into(
                            &mut introduced_bindings,
                            still_syntax_node_as_ref(&case.pattern),
                        );
                        local_bindings.push((
                            still_syntax_node_as_ref(case_result_node),
                            introduced_bindings,
                        ));
                        still_syntax_expression_find_variable_at_position(
                            local_bindings,
                            scope_declaration,
                            still_syntax_node_as_ref(case_result_node),
                            position,
                        )
                    } else {
                        std::ops::ControlFlow::Continue(local_bindings)
                    }
                })
        }
        StillSyntaxExpression::Char(_) => std::ops::ControlFlow::Continue(local_bindings),
        StillSyntaxExpression::Dec(_) => std::ops::ControlFlow::Continue(local_bindings),
        StillSyntaxExpression::Int { .. } => std::ops::ControlFlow::Continue(local_bindings),
        StillSyntaxExpression::Lambda {
            arrow_key_symbol_range: _,
            parameter: maybe_parameter,
            result: maybe_result,
        } => {
            if let Some(found_symbol) = maybe_parameter.iter().find_map(|parameter| {
                still_syntax_pattern_find_variable_at_position(
                    scope_declaration,
                    still_syntax_node_as_ref(parameter),
                    position,
                )
            }) {
                return std::ops::ControlFlow::Break(found_symbol);
            }
            match maybe_result {
                Some(result_node) => {
                    let mut introduced_bindings: Vec<StillLocalBinding> = Vec::new();
                    if let Some(parameter_node) = maybe_parameter {
                        still_syntax_pattern_bindings_into(
                            &mut introduced_bindings,
                            still_syntax_node_as_ref(parameter_node),
                        );
                    }
                    local_bindings
                        .push((still_syntax_node_unbox(result_node), introduced_bindings));
                    still_syntax_expression_find_variable_at_position(
                        local_bindings,
                        scope_declaration,
                        still_syntax_node_unbox(result_node),
                        position,
                    )
                }
                None => std::ops::ControlFlow::Continue(local_bindings),
            }
        }
        StillSyntaxExpression::Let {
            declaration: declarations,
            result: maybe_result,
        } => {
            let mut introduced_bindings: Vec<StillLocalBinding> = Vec::new();
            if let Some(let_declaration_node) = declarations {
                still_syntax_let_declaration_introduced_bindings_into(
                    &mut introduced_bindings,
                    &let_declaration_node.value,
                );
            }
            local_bindings.push((still_syntax_expression_node, introduced_bindings));
            local_bindings =
                declarations
                    .iter()
                    .try_fold(local_bindings, |local_bindings, declaration| {
                        still_syntax_let_declaration_find_variable_at_position(
                            local_bindings,
                            scope_declaration,
                            still_syntax_expression_node,
                            still_syntax_node_as_ref(declaration),
                            position,
                        )
                    })?;
            match maybe_result {
                Some(result_node) => still_syntax_expression_find_variable_at_position(
                    local_bindings,
                    scope_declaration,
                    still_syntax_node_unbox(result_node),
                    position,
                ),
                None => std::ops::ControlFlow::Continue(local_bindings),
            }
        }
        StillSyntaxExpression::Vec(elements) => {
            elements
                .iter()
                .try_fold(local_bindings, |local_bindings, element| {
                    still_syntax_expression_find_variable_at_position(
                        local_bindings,
                        scope_declaration,
                        still_syntax_node_as_ref(element),
                        position,
                    )
                })
        }
        StillSyntaxExpression::Parenthesized(None) => {
            std::ops::ControlFlow::Continue(local_bindings)
        }
        StillSyntaxExpression::Parenthesized(Some(in_parens)) => {
            still_syntax_expression_find_variable_at_position(
                local_bindings,
                scope_declaration,
                still_syntax_node_unbox(in_parens),
                position,
            )
        }
        StillSyntaxExpression::Typed {
            type_: maybe_type,
            expression: maybe_expression_in_typed,
        } => {
            if let Some(found) = maybe_type.as_ref().and_then(|type_node| {
                still_syntax_type_find_variable_at_position(
                    scope_declaration,
                    still_syntax_node_as_ref(type_node),
                    position,
                )
            }) {
                return std::ops::ControlFlow::Break(found);
            }
            match maybe_expression_in_typed {
                None => std::ops::ControlFlow::Continue(local_bindings),
                Some(expression_node_in_typed) => match &expression_node_in_typed.value {
                    StillSyntaxExpressionUntyped::Variant {
                        name: name_node,
                        value: maybe_value,
                    } => {
                        if lsp_range_includes_position(name_node.range, position) {
                            return std::ops::ControlFlow::Break(StillSyntaxNode {
                                value: StillSyntaxSymbol::VariableOrVariant {
                                    name: &name_node.value,
                                    local_bindings: local_bindings,
                                },
                                range: still_syntax_expression_node.range,
                            });
                        }
                        match maybe_value {
                            Some(value_node) => still_syntax_expression_find_variable_at_position(
                                local_bindings,
                                scope_declaration,
                                still_syntax_node_unbox(value_node),
                                position,
                            ),
                            None => std::ops::ControlFlow::Continue(local_bindings),
                        }
                    }
                    StillSyntaxExpressionUntyped::Other(other_expression_in_typed) => {
                        still_syntax_expression_find_variable_at_position(
                            local_bindings,
                            scope_declaration,
                            StillSyntaxNode {
                                range: expression_node_in_typed.range,
                                value: other_expression_in_typed,
                            },
                            position,
                        )
                    }
                },
            }
        }
        StillSyntaxExpression::Record(fields) => {
            fields
                .iter()
                .try_fold(local_bindings, |local_bindings, field| match &field.value {
                    Some(field_value_node) => still_syntax_expression_find_variable_at_position(
                        local_bindings,
                        scope_declaration,
                        still_syntax_node_as_ref(field_value_node),
                        position,
                    ),
                    None => std::ops::ControlFlow::Continue(local_bindings),
                })
        }
        StillSyntaxExpression::RecordAccess { record, field: _ } => {
            still_syntax_expression_find_variable_at_position(
                local_bindings,
                scope_declaration,
                still_syntax_node_unbox(record),
                position,
            )
        }
        StillSyntaxExpression::RecordUpdate {
            record: maybe_record,
            spread_key_symbol_range: _,
            fields,
        } => {
            if let Some(record_node) = maybe_record
                && lsp_range_includes_position(record_node.range, position)
            {
                return still_syntax_expression_find_variable_at_position(
                    local_bindings,
                    scope_declaration,
                    still_syntax_node_unbox(record_node),
                    position,
                );
            }
            fields
                .iter()
                .try_fold(local_bindings, |local_bindings, field| match &field.value {
                    Some(field_value_node) => still_syntax_expression_find_variable_at_position(
                        local_bindings,
                        scope_declaration,
                        still_syntax_node_as_ref(field_value_node),
                        position,
                    ),
                    None => std::ops::ControlFlow::Continue(local_bindings),
                })
        }
        StillSyntaxExpression::String { .. } => std::ops::ControlFlow::Continue(local_bindings),
    }
}

fn still_syntax_let_declaration_find_variable_at_position<'a>(
    local_bindings: StillLocalBindings<'a>,
    scope_declaration: &'a StillSyntaxDeclaration,
    scope_expression: StillSyntaxNode<&'a StillSyntaxExpression>,
    still_syntax_let_declaration_node: StillSyntaxNode<&'a StillSyntaxLetDeclaration>,
    position: lsp_types::Position,
) -> std::ops::ControlFlow<StillSyntaxNode<StillSyntaxSymbol<'a>>, StillLocalBindings<'a>> {
    if !lsp_range_includes_position(still_syntax_let_declaration_node.range, position) {
        return std::ops::ControlFlow::Continue(local_bindings);
    }
    match still_syntax_let_declaration_node.value {
        StillSyntaxLetDeclaration::Destructuring {
            pattern,
            equals_key_symbol_range: _,
            expression: maybe_expression,
        } => {
            on_some_break(still_syntax_pattern_find_variable_at_position(
                scope_declaration,
                still_syntax_node_as_ref(pattern),
                position,
            ))?;
            match maybe_expression {
                Some(expression_node) => still_syntax_expression_find_variable_at_position(
                    local_bindings,
                    scope_declaration,
                    still_syntax_node_unbox(expression_node),
                    position,
                ),
                None => std::ops::ControlFlow::Continue(local_bindings),
            }
        }
        StillSyntaxLetDeclaration::VariableDeclaration {
            start_name,
            result: maybe_result_node,
        } => {
            if lsp_range_includes_position(start_name.range, position) {
                return std::ops::ControlFlow::Break(StillSyntaxNode {
                    value: StillSyntaxSymbol::LetDeclarationName {
                        name: &start_name.value,
                        start_name_range: start_name.range,
                        type_type: maybe_result_node.as_ref().and_then(|result_node| {
                            still_syntax_expression_type(still_syntax_node_unbox(result_node)).ok()
                        }),
                        scope_expression: scope_expression,
                    },
                    range: start_name.range,
                });
            }
            match maybe_result_node {
                Some(result_node) => still_syntax_expression_find_variable_at_position(
                    local_bindings,
                    scope_declaration,
                    still_syntax_node_unbox(result_node),
                    position,
                ),
                None => std::ops::ControlFlow::Continue(local_bindings),
            }
        }
    }
}

// //
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum StillSymbolToReference<'a> {
    TypeVariable(&'a str),
    // type is tracked separately from VariableOrVariant because e.g. variants and
    // type names are allowed to overlap
    Type {
        name: &'a str,
        including_declaration_name: bool,
    },
    VariableOrVariant {
        name: &'a str,
        including_declaration_name: bool,
    },
    LocalBinding {
        name: &'a str,
        including_let_declaration_name: bool,
    },
}

fn still_syntax_project_uses_of_variable_into(
    uses_so_far: &mut Vec<lsp_types::Range>,
    still_syntax_project: &StillSyntaxProject,
    symbol_to_collect_uses_of: StillSymbolToReference,
) {
    for documented_declaration in still_syntax_project
        .declarations
        .iter()
        .filter_map(|declaration_or_err| declaration_or_err.as_ref().ok())
    {
        if let Some(declaration_node) = &documented_declaration.declaration {
            still_syntax_declaration_uses_of_variable_into(
                uses_so_far,
                &declaration_node.value,
                symbol_to_collect_uses_of,
            );
        }
    }
}

fn still_syntax_declaration_uses_of_variable_into(
    uses_so_far: &mut Vec<lsp_types::Range>,
    still_syntax_declaration: &StillSyntaxDeclaration,
    symbol_to_collect_uses_of: StillSymbolToReference,
) {
    match still_syntax_declaration {
        StillSyntaxDeclaration::ChoiceType {
            name: maybe_name,
            parameters,
            equals_key_symbol_range: _,
            variant0_name: maybe_variant0_name,
            variant0_value: variant0_maybe_value,
            variant1_up,
        } => {
            if let Some(name_node) = maybe_name
                && symbol_to_collect_uses_of
                    == (StillSymbolToReference::Type {
                        name: &name_node.value,
                        including_declaration_name: true,
                    })
            {
                uses_so_far.push(name_node.range);
            }
            'parameter_traversal: for parameter_node in parameters {
                if symbol_to_collect_uses_of
                    == StillSymbolToReference::TypeVariable(&parameter_node.value)
                {
                    uses_so_far.push(parameter_node.range);
                    break 'parameter_traversal;
                }
            }
            if let Some(variant0_name_node) = maybe_variant0_name
                && symbol_to_collect_uses_of
                    == (StillSymbolToReference::VariableOrVariant {
                        name: &variant0_name_node.value,
                        including_declaration_name: true,
                    })
            {
                uses_so_far.push(variant0_name_node.range);
                return;
            }
            if let Some(variant0_value) = variant0_maybe_value {
                still_syntax_type_uses_of_variable_into(
                    uses_so_far,
                    still_syntax_node_as_ref(variant0_value),
                    symbol_to_collect_uses_of,
                );
            }
            for variant in variant1_up {
                if let Some(variant_name_node) = &variant.name
                    && (StillSymbolToReference::VariableOrVariant {
                        name: &variant_name_node.value,

                        including_declaration_name: true,
                    }) == symbol_to_collect_uses_of
                {
                    uses_so_far.push(variant_name_node.range);
                    return;
                }
                for variant0_value in variant.value.iter() {
                    still_syntax_type_uses_of_variable_into(
                        uses_so_far,
                        still_syntax_node_as_ref(variant0_value),
                        symbol_to_collect_uses_of,
                    );
                }
            }
        }
        StillSyntaxDeclaration::TypeAlias {
            alias_keyword_range: _,
            name: maybe_name,
            parameters,
            equals_key_symbol_range: _,
            type_: maybe_type,
        } => {
            if let Some(name_node) = maybe_name
                && (symbol_to_collect_uses_of
                    == (StillSymbolToReference::Type {
                        name: &name_node.value,

                        including_declaration_name: true,
                    }))
            {
                uses_so_far.push(name_node.range);
            }
            'parameter_traversal: for parameter_node in parameters {
                if symbol_to_collect_uses_of
                    == StillSymbolToReference::TypeVariable(&parameter_node.value)
                {
                    uses_so_far.push(parameter_node.range);
                    break 'parameter_traversal;
                }
            }
            if let Some(type_node) = maybe_type {
                still_syntax_type_uses_of_variable_into(
                    uses_so_far,
                    still_syntax_node_as_ref(type_node),
                    symbol_to_collect_uses_of,
                );
            }
        }
        StillSyntaxDeclaration::Variable {
            start_name: start_name_node,
            result: maybe_result,
        } => {
            if symbol_to_collect_uses_of
                == (StillSymbolToReference::VariableOrVariant {
                    name: &start_name_node.value,

                    including_declaration_name: true,
                })
            {
                uses_so_far.push(start_name_node.range);
            }
            if let Some(result_node) = maybe_result {
                still_syntax_expression_uses_of_variable_into(
                    uses_so_far,
                    &[],
                    still_syntax_node_as_ref(result_node),
                    symbol_to_collect_uses_of,
                );
            }
        }
    }
}

fn still_syntax_type_uses_of_variable_into(
    uses_so_far: &mut Vec<lsp_types::Range>,
    still_syntax_type_node: StillSyntaxNode<&StillSyntaxType>,
    symbol_to_collect_uses_of: StillSymbolToReference,
) {
    match still_syntax_type_node.value {
        StillSyntaxType::Construct {
            name: variable,
            arguments,
        } => {
            if let StillSymbolToReference::Type {
                name: symbol_name,
                including_declaration_name: _,
            } = symbol_to_collect_uses_of
                && symbol_name == variable.value.as_str()
            {
                uses_so_far.push(lsp_types::Range {
                    start: lsp_position_add_characters(
                        variable.range.end,
                        -(variable.value.len() as i32),
                    ),
                    end: variable.range.end,
                });
            }
            for argument in arguments {
                still_syntax_type_uses_of_variable_into(
                    uses_so_far,
                    still_syntax_node_as_ref(argument),
                    symbol_to_collect_uses_of,
                );
            }
        }
        StillSyntaxType::Function {
            input: maybe_input,
            arrow_key_symbol_range: _,
            output: maybe_output,
        } => {
            if let Some(input) = maybe_input {
                still_syntax_type_uses_of_variable_into(
                    uses_so_far,
                    still_syntax_node_unbox(input),
                    symbol_to_collect_uses_of,
                );
            }
            if let Some(output_node) = maybe_output {
                still_syntax_type_uses_of_variable_into(
                    uses_so_far,
                    still_syntax_node_unbox(output_node),
                    symbol_to_collect_uses_of,
                );
            }
        }
        StillSyntaxType::Parenthesized(None) => {}
        StillSyntaxType::Parenthesized(Some(in_parens)) => {
            still_syntax_type_uses_of_variable_into(
                uses_so_far,
                still_syntax_node_unbox(in_parens),
                symbol_to_collect_uses_of,
            );
        }
        StillSyntaxType::Record(fields) => {
            for field in fields {
                if let Some(field_value_node) = &field.value {
                    still_syntax_type_uses_of_variable_into(
                        uses_so_far,
                        still_syntax_node_as_ref(field_value_node),
                        symbol_to_collect_uses_of,
                    );
                }
            }
        }
        StillSyntaxType::Variable(variable) => {
            if symbol_to_collect_uses_of == StillSymbolToReference::TypeVariable(variable) {
                uses_so_far.push(still_syntax_type_node.range);
            }
        }
    }
}

fn still_syntax_expression_uses_of_variable_into(
    uses_so_far: &mut Vec<lsp_types::Range>,
    local_bindings: &[StillLocalBinding],
    still_syntax_expression_node: StillSyntaxNode<&StillSyntaxExpression>,
    symbol_to_collect_uses_of: StillSymbolToReference,
) {
    match still_syntax_expression_node.value {
        StillSyntaxExpression::VariableOrCall {
            variable: variable_node,
            arguments,
        } => {
            let name: &str = variable_node.value.as_str();
            if let StillSymbolToReference::LocalBinding {
                name: symbol_name,
                including_let_declaration_name: _,
            } = symbol_to_collect_uses_of
                && symbol_name == name
                && local_bindings
                    .iter()
                    .any(|local_binding| local_binding.name == name)
            {
                uses_so_far.push(still_syntax_expression_node.range);
            }
            for argument_node in arguments {
                still_syntax_expression_uses_of_variable_into(
                    uses_so_far,
                    local_bindings,
                    still_syntax_node_as_ref(argument_node),
                    symbol_to_collect_uses_of,
                );
            }
        }
        StillSyntaxExpression::CaseOf {
            matched: maybe_matched,
            of_keyword_range: _,
            cases,
        } => {
            if let Some(matched_node) = maybe_matched {
                still_syntax_expression_uses_of_variable_into(
                    uses_so_far,
                    local_bindings,
                    still_syntax_node_unbox(matched_node),
                    symbol_to_collect_uses_of,
                );
            }
            for case in cases {
                still_syntax_pattern_uses_of_variable_into(
                    uses_so_far,
                    still_syntax_node_as_ref(&case.pattern),
                    symbol_to_collect_uses_of,
                );
                if let Some(case_result_node) = &case.result {
                    let mut local_bindings_including_from_case_pattern: Vec<StillLocalBinding> =
                        local_bindings.to_vec();
                    still_syntax_pattern_bindings_into(
                        &mut local_bindings_including_from_case_pattern,
                        still_syntax_node_as_ref(&case.pattern),
                    );
                    still_syntax_expression_uses_of_variable_into(
                        uses_so_far,
                        &local_bindings_including_from_case_pattern,
                        still_syntax_node_as_ref(case_result_node),
                        symbol_to_collect_uses_of,
                    );
                }
            }
        }
        StillSyntaxExpression::Char(_) => {}
        StillSyntaxExpression::Dec(_) => {}
        StillSyntaxExpression::Int { .. } => {}
        StillSyntaxExpression::Lambda {
            parameter: maybe_parameter,
            arrow_key_symbol_range: _,
            result: maybe_result,
        } => {
            if let Some(parameter_node) = maybe_parameter {
                still_syntax_pattern_uses_of_variable_into(
                    uses_so_far,
                    still_syntax_node_as_ref(parameter_node),
                    symbol_to_collect_uses_of,
                );
            }
            if let Some(result_node) = maybe_result {
                let mut local_bindings_including_from_lambda_parameters = local_bindings.to_vec();
                if let Some(parameter_node) = maybe_parameter {
                    still_syntax_pattern_bindings_into(
                        &mut local_bindings_including_from_lambda_parameters,
                        still_syntax_node_as_ref(parameter_node),
                    );
                }
                still_syntax_expression_uses_of_variable_into(
                    uses_so_far,
                    &local_bindings_including_from_lambda_parameters,
                    still_syntax_node_unbox(result_node),
                    symbol_to_collect_uses_of,
                );
            }
        }
        StillSyntaxExpression::Let {
            declaration: maybe_declaration,
            result: maybe_result,
        } => {
            let mut local_bindings_including_let_declaration_introduced: Vec<StillLocalBinding> =
                local_bindings.to_vec();
            while let Some(let_declaration_node) = maybe_declaration {
                still_syntax_let_declaration_introduced_bindings_into(
                    &mut local_bindings_including_let_declaration_introduced,
                    &let_declaration_node.value,
                );
            }
            if let Some(let_declaration_node) = maybe_declaration {
                still_syntax_let_declaration_uses_of_variable_into(
                    uses_so_far,
                    &local_bindings_including_let_declaration_introduced,
                    &let_declaration_node.value,
                    symbol_to_collect_uses_of,
                );
            }
            if let Some(result) = maybe_result {
                still_syntax_expression_uses_of_variable_into(
                    uses_so_far,
                    &local_bindings_including_let_declaration_introduced,
                    still_syntax_node_unbox(result),
                    symbol_to_collect_uses_of,
                );
            }
        }
        StillSyntaxExpression::Vec(elements) => {
            for element_node in elements {
                still_syntax_expression_uses_of_variable_into(
                    uses_so_far,
                    local_bindings,
                    still_syntax_node_as_ref(element_node),
                    symbol_to_collect_uses_of,
                );
            }
        }
        StillSyntaxExpression::Parenthesized(None) => {}
        StillSyntaxExpression::Parenthesized(Some(in_parens)) => {
            still_syntax_expression_uses_of_variable_into(
                uses_so_far,
                local_bindings,
                still_syntax_node_unbox(in_parens),
                symbol_to_collect_uses_of,
            );
        }
        StillSyntaxExpression::Typed {
            type_: maybe_type,
            expression: maybe_expression_in_typed,
        } => {
            if let Some(type_node) = maybe_type {
                still_syntax_type_uses_of_variable_into(
                    uses_so_far,
                    still_syntax_node_as_ref(type_node),
                    symbol_to_collect_uses_of,
                );
            }
            if let Some(expression_node_in_typed) = maybe_expression_in_typed {
                match &expression_node_in_typed.value {
                    StillSyntaxExpressionUntyped::Variant {
                        name: name_node,
                        value: maybe_value,
                    } => {
                        if let StillSymbolToReference::VariableOrVariant {
                            name: symbol_name,
                            including_declaration_name: _,
                        } = symbol_to_collect_uses_of
                            && symbol_name == name_node.value.as_str()
                        {
                            uses_so_far.push(lsp_types::Range {
                                start: lsp_position_add_characters(
                                    still_syntax_expression_node.range.end,
                                    -(name_node.value.len() as i32),
                                ),
                                end: still_syntax_expression_node.range.end,
                            });
                        }
                        if let Some(value_node) = maybe_value {
                            still_syntax_expression_uses_of_variable_into(
                                uses_so_far,
                                local_bindings,
                                still_syntax_node_unbox(value_node),
                                symbol_to_collect_uses_of,
                            );
                        }
                    }
                    StillSyntaxExpressionUntyped::Other(other_expression_in_typed) => {
                        still_syntax_expression_uses_of_variable_into(
                            uses_so_far,
                            local_bindings,
                            StillSyntaxNode {
                                range: expression_node_in_typed.range,
                                value: other_expression_in_typed,
                            },
                            symbol_to_collect_uses_of,
                        );
                    }
                }
            }
        }
        StillSyntaxExpression::Record(fields) => {
            for field in fields {
                if let Some(field_value_node) = &field.value {
                    still_syntax_expression_uses_of_variable_into(
                        uses_so_far,
                        local_bindings,
                        still_syntax_node_as_ref(field_value_node),
                        symbol_to_collect_uses_of,
                    );
                }
            }
        }
        StillSyntaxExpression::RecordAccess { record, field: _ } => {
            still_syntax_expression_uses_of_variable_into(
                uses_so_far,
                local_bindings,
                still_syntax_node_unbox(record),
                symbol_to_collect_uses_of,
            );
        }
        StillSyntaxExpression::RecordUpdate {
            record: maybe_record,
            spread_key_symbol_range: _,
            fields,
        } => {
            if let Some(record_node) = maybe_record {
                still_syntax_expression_uses_of_variable_into(
                    uses_so_far,
                    local_bindings,
                    still_syntax_node_unbox(record_node),
                    symbol_to_collect_uses_of,
                );
            }
            for field in fields {
                if let Some(field_value_node) = &field.value {
                    still_syntax_expression_uses_of_variable_into(
                        uses_so_far,
                        local_bindings,
                        still_syntax_node_as_ref(field_value_node),
                        symbol_to_collect_uses_of,
                    );
                }
            }
        }
        StillSyntaxExpression::String { .. } => {}
    }
}

fn still_syntax_let_declaration_uses_of_variable_into(
    uses_so_far: &mut Vec<lsp_types::Range>,

    local_bindings: &[StillLocalBinding],
    still_syntax_let_declaration: &StillSyntaxLetDeclaration,
    symbol_to_collect_uses_of: StillSymbolToReference,
) {
    match still_syntax_let_declaration {
        StillSyntaxLetDeclaration::Destructuring {
            pattern,
            equals_key_symbol_range: _,
            expression: maybe_expression,
        } => {
            still_syntax_pattern_uses_of_variable_into(
                uses_so_far,
                still_syntax_node_as_ref(pattern),
                symbol_to_collect_uses_of,
            );
            if let Some(expression_node) = maybe_expression {
                still_syntax_expression_uses_of_variable_into(
                    uses_so_far,
                    local_bindings,
                    still_syntax_node_unbox(expression_node),
                    symbol_to_collect_uses_of,
                );
            }
        }
        StillSyntaxLetDeclaration::VariableDeclaration {
            start_name: start_name_node,
            result: maybe_result,
        } => {
            if symbol_to_collect_uses_of
                == (StillSymbolToReference::LocalBinding {
                    name: &start_name_node.value,
                    including_let_declaration_name: true,
                })
            {
                uses_so_far.push(start_name_node.range);
                return;
            }
            if let Some(result_node) = maybe_result {
                still_syntax_expression_uses_of_variable_into(
                    uses_so_far,
                    local_bindings,
                    still_syntax_node_unbox(result_node),
                    symbol_to_collect_uses_of,
                );
            }
        }
    }
}

fn still_syntax_pattern_uses_of_variable_into(
    uses_so_far: &mut Vec<lsp_types::Range>,
    still_syntax_pattern_node: StillSyntaxNode<&StillSyntaxPattern>,
    symbol_to_collect_uses_of: StillSymbolToReference,
) {
    match still_syntax_pattern_node.value {
        StillSyntaxPattern::Char(_) => {}
        StillSyntaxPattern::Int { .. } => {}
        StillSyntaxPattern::Typed {
            type_: maybe_type_node,
            pattern: maybe_pattern_node_in_typed,
        } => {
            if let Some(type_node) = maybe_type_node {
                still_syntax_type_uses_of_variable_into(
                    uses_so_far,
                    still_syntax_node_as_ref(type_node),
                    symbol_to_collect_uses_of,
                );
            }
            if let Some(pattern_node_in_typed) = maybe_pattern_node_in_typed {
                match &pattern_node_in_typed.value {
                    StillSyntaxPatternUntyped::Ignored => {}
                    StillSyntaxPatternUntyped::Variable(_) => {}
                    StillSyntaxPatternUntyped::Variant {
                        name: variable,
                        value: maybe_value,
                    } => {
                        if let StillSymbolToReference::VariableOrVariant {
                            name: symbol_name,
                            including_declaration_name: _,
                        } = symbol_to_collect_uses_of
                            && symbol_name == variable.value.as_str()
                        {
                            uses_so_far.push(lsp_types::Range {
                                start: lsp_position_add_characters(
                                    variable.range.end,
                                    -(variable.value.len() as i32),
                                ),
                                end: variable.range.end,
                            });
                        }
                        if let Some(value) = maybe_value {
                            still_syntax_pattern_uses_of_variable_into(
                                uses_so_far,
                                still_syntax_node_unbox(value),
                                symbol_to_collect_uses_of,
                            );
                        }
                    }
                }
            }
        }
        StillSyntaxPattern::Record(_) => {}
        StillSyntaxPattern::String { .. } => {}
    }
}

fn still_syntax_let_declaration_introduced_bindings_into<'a>(
    bindings_so_far: &mut Vec<StillLocalBinding<'a>>,
    still_syntax_let_declaration: &'a StillSyntaxLetDeclaration,
) {
    match still_syntax_let_declaration {
        StillSyntaxLetDeclaration::Destructuring { pattern, .. } => {
            still_syntax_pattern_bindings_into(bindings_so_far, still_syntax_node_as_ref(pattern));
        }
        StillSyntaxLetDeclaration::VariableDeclaration {
            start_name: start_name_node,
            result: maybe_result_node,
        } => {
            bindings_so_far.push(StillLocalBinding {
                name: &start_name_node.value,
                origin: LocalBindingOrigin::LetDeclaredVariable {
                    type_: maybe_result_node.as_ref().and_then(|result_node| {
                        still_syntax_expression_type(still_syntax_node_unbox(result_node)).ok()
                    }),
                    name_range: start_name_node.range,
                },
            });
        }
    }
}

fn still_syntax_pattern_bindings_into<'a>(
    bindings_so_far: &mut Vec<StillLocalBinding<'a>>,
    still_syntax_pattern_node: StillSyntaxNode<&'a StillSyntaxPattern>,
) {
    match still_syntax_pattern_node.value {
        StillSyntaxPattern::Char(_) => {}
        StillSyntaxPattern::Int { .. } => {}
        StillSyntaxPattern::Typed {
            type_: _,
            pattern: maybe_pattern_node_in_typed,
        } => {
            if let Some(pattern_node_in_typed) = maybe_pattern_node_in_typed {
                match &pattern_node_in_typed.value {
                    StillSyntaxPatternUntyped::Ignored => {}
                    StillSyntaxPatternUntyped::Variable(variable) => {
                        bindings_so_far.push(StillLocalBinding {
                            origin: LocalBindingOrigin::PatternVariable(
                                pattern_node_in_typed.range,
                            ),
                            name: variable,
                        });
                    }
                    StillSyntaxPatternUntyped::Variant {
                        name: _,
                        value: maybe_value,
                    } => {
                        if let Some(value_node) = maybe_value {
                            still_syntax_pattern_bindings_into(
                                bindings_so_far,
                                still_syntax_node_unbox(value_node),
                            );
                        }
                    }
                }
            }
        }
        StillSyntaxPattern::Record(fields) => {
            for field in fields {
                if let Some(field_value_node) = &field.value {
                    still_syntax_pattern_bindings_into(
                        bindings_so_far,
                        still_syntax_node_as_ref(field_value_node),
                    );
                }
            }
        }
        StillSyntaxPattern::String { .. } => {}
    }
}

enum StillSyntaxHighlightKind {
    Type,
    TypeVariable,
    Variant,
    Field,
    Variable,
    Comment,
    String,
    Number,
    DeclaredVariable,
    KeySymbol,
}

fn still_syntax_highlight_project_into(
    highlighted_so_far: &mut Vec<StillSyntaxNode<StillSyntaxHighlightKind>>,
    still_syntax_project: &StillSyntaxProject,
) {
    for documented_declaration in still_syntax_project
        .declarations
        .iter()
        .filter_map(|declaration_or_err| declaration_or_err.as_ref().ok())
    {
        if let Some(documentation_node) = &documented_declaration.documentation {
            highlighted_so_far.extend(
                still_syntax_highlight_multi_line(
                    still_syntax_node_unbox(documentation_node),
                    3,
                    2,
                )
                .map(|range| StillSyntaxNode {
                    range: range,
                    value: StillSyntaxHighlightKind::Comment,
                }),
            );
        }
        if let Some(declaration_node) = &documented_declaration.declaration {
            still_syntax_highlight_declaration_into(
                highlighted_so_far,
                still_syntax_node_as_ref(declaration_node),
            );
        }
    }
    // Inserting many comments in the middle can get expensive (having so many comments to make it matter will be rare).
    // A possible solution (when comment count exceeds other syntax by some factor) is just pushing all comments an sorting the whole thing at once.
    // Feels like overkill, though so I'll hold on on this until issues are opened :)
    for comment_node in still_syntax_project.comments.iter() {
        still_syntax_highlight_and_place_comment_into(
            highlighted_so_far,
            still_syntax_node_unbox(comment_node),
        );
    }
}
fn still_syntax_highlight_and_place_comment_into(
    highlighted_so_far: &mut Vec<StillSyntaxNode<StillSyntaxHighlightKind>>,
    still_syntax_comment_node: StillSyntaxNode<&str>,
) {
    let insert_index: usize = highlighted_so_far
        .binary_search_by(|token| {
            token
                .range
                .start
                .cmp(&still_syntax_comment_node.range.start)
        })
        .unwrap_or_else(|i| i);
    highlighted_so_far.insert(
        insert_index,
        StillSyntaxNode {
            range: still_syntax_comment_node.range,
            value: StillSyntaxHighlightKind::Comment,
        },
    );
}
fn still_syntax_highlight_multi_line(
    still_syntax_str_node: StillSyntaxNode<&str>,
    characters_before_content: usize,
    characters_after_content: usize,
) -> impl Iterator<Item = lsp_types::Range> {
    let content_does_not_break_line: bool =
        still_syntax_str_node.range.start.line == still_syntax_str_node.range.end.line;
    still_syntax_str_node
        .value
        .lines()
        .chain(
            // str::lines() eats the last linebreak. Restore it
            if still_syntax_str_node.value.ends_with("\n") {
                Some("\n")
            } else {
                None
            },
        )
        .enumerate()
        .map(move |(inner_line, inner_line_str)| {
            let line: u32 = still_syntax_str_node.range.start.line + (inner_line as u32);
            let line_length_utf16: usize = inner_line_str.encode_utf16().count();
            if inner_line == 0 {
                lsp_types::Range {
                    start: still_syntax_str_node.range.start,
                    end: lsp_position_add_characters(
                        still_syntax_str_node.range.start,
                        (characters_before_content
                            + line_length_utf16
                            + if content_does_not_break_line {
                                characters_after_content
                            } else {
                                0
                            }) as i32,
                    ),
                }
            } else {
                lsp_types::Range {
                    start: lsp_types::Position {
                        line: line,
                        character: 0,
                    },
                    end: if line == still_syntax_str_node.range.end.line {
                        still_syntax_str_node.range.end
                    } else {
                        lsp_types::Position {
                            line: line,
                            character: (line_length_utf16 + characters_after_content) as u32,
                        }
                    },
                }
            }
        })
}

fn still_syntax_highlight_declaration_into(
    highlighted_so_far: &mut Vec<StillSyntaxNode<StillSyntaxHighlightKind>>,
    still_syntax_declaration_node: StillSyntaxNode<&StillSyntaxDeclaration>,
) {
    match still_syntax_declaration_node.value {
        StillSyntaxDeclaration::Variable {
            start_name: start_name_node,
            result: maybe_result,
        } => {
            highlighted_so_far.push(StillSyntaxNode {
                range: start_name_node.range,
                value: StillSyntaxHighlightKind::DeclaredVariable,
            });
            if let Some(result_node) = maybe_result {
                still_syntax_highlight_expression_into(
                    highlighted_so_far,
                    &[],
                    still_syntax_node_as_ref(result_node),
                );
            }
        }
        StillSyntaxDeclaration::ChoiceType {
            name: maybe_name,
            parameters,
            equals_key_symbol_range: maybe_equals_key_symbol_range,
            variant0_name: maybe_variant0_name,
            variant0_value: variant0_maybe_value,
            variant1_up,
        } => {
            highlighted_so_far.push(StillSyntaxNode {
                range: lsp_types::Range {
                    start: still_syntax_declaration_node.range.start,
                    end: lsp_position_add_characters(still_syntax_declaration_node.range.start, 4),
                },
                value: StillSyntaxHighlightKind::KeySymbol,
            });
            if let Some(name_node) = maybe_name {
                highlighted_so_far.push(StillSyntaxNode {
                    range: name_node.range,
                    value: StillSyntaxHighlightKind::Type,
                });
            }
            for parameter_name_node in parameters {
                highlighted_so_far.push(StillSyntaxNode {
                    range: parameter_name_node.range,
                    value: StillSyntaxHighlightKind::TypeVariable,
                });
            }
            if let &Some(equals_key_symbol_range) = maybe_equals_key_symbol_range {
                highlighted_so_far.push(StillSyntaxNode {
                    range: equals_key_symbol_range,
                    value: StillSyntaxHighlightKind::KeySymbol,
                });
            }
            if let Some(variant0_name_node) = maybe_variant0_name {
                highlighted_so_far.push(StillSyntaxNode {
                    range: variant0_name_node.range,
                    value: StillSyntaxHighlightKind::Variant,
                });
            }
            if let Some(variant0_value_node) = variant0_maybe_value {
                still_syntax_highlight_type_into(
                    highlighted_so_far,
                    still_syntax_node_as_ref(variant0_value_node),
                );
            }
            for variant in variant1_up {
                highlighted_so_far.push(StillSyntaxNode {
                    range: variant.or_key_symbol_range,
                    value: StillSyntaxHighlightKind::KeySymbol,
                });
                if let Some(variant_name_node) = &variant.name {
                    highlighted_so_far.push(StillSyntaxNode {
                        range: variant_name_node.range,
                        value: StillSyntaxHighlightKind::Variant,
                    });
                }
                for variant_value_node in variant.value.iter() {
                    still_syntax_highlight_type_into(
                        highlighted_so_far,
                        still_syntax_node_as_ref(variant_value_node),
                    );
                }
            }
        }
        StillSyntaxDeclaration::TypeAlias {
            alias_keyword_range,
            name: maybe_name,
            parameters,
            equals_key_symbol_range: maybe_equals_key_symbol_range,
            type_: maybe_type,
        } => {
            highlighted_so_far.push(StillSyntaxNode {
                range: lsp_types::Range {
                    start: still_syntax_declaration_node.range.start,
                    end: lsp_position_add_characters(still_syntax_declaration_node.range.start, 4),
                },
                value: StillSyntaxHighlightKind::KeySymbol,
            });
            highlighted_so_far.push(StillSyntaxNode {
                range: *alias_keyword_range,
                value: StillSyntaxHighlightKind::KeySymbol,
            });
            if let Some(name_node) = maybe_name {
                highlighted_so_far.push(StillSyntaxNode {
                    range: name_node.range,
                    value: StillSyntaxHighlightKind::Type,
                });
            }
            for parameter_name_node in parameters {
                highlighted_so_far.push(StillSyntaxNode {
                    range: parameter_name_node.range,
                    value: StillSyntaxHighlightKind::TypeVariable,
                });
            }
            if let &Some(equals_key_symbol_range) = maybe_equals_key_symbol_range {
                highlighted_so_far.push(StillSyntaxNode {
                    range: equals_key_symbol_range,
                    value: StillSyntaxHighlightKind::KeySymbol,
                });
            }
            if let Some(type_node) = maybe_type {
                still_syntax_highlight_type_into(
                    highlighted_so_far,
                    still_syntax_node_as_ref(type_node),
                );
            }
        }
    }
}

fn still_syntax_highlight_pattern_into(
    highlighted_so_far: &mut Vec<StillSyntaxNode<StillSyntaxHighlightKind>>,
    still_syntax_pattern_node: StillSyntaxNode<&StillSyntaxPattern>,
) {
    match still_syntax_pattern_node.value {
        StillSyntaxPattern::Char(_) => {
            highlighted_so_far.push(StillSyntaxNode {
                range: still_syntax_pattern_node.range,
                value: StillSyntaxHighlightKind::String,
            });
        }
        StillSyntaxPattern::Int { .. } => {
            highlighted_so_far.push(StillSyntaxNode {
                range: still_syntax_pattern_node.range,
                value: StillSyntaxHighlightKind::Number,
            });
        }
        StillSyntaxPattern::Typed {
            type_: maybe_type_node,
            pattern: maybe_pattern_node_in_typed,
        } => {
            if let Some(type_node) = maybe_type_node {
                still_syntax_highlight_type_into(
                    highlighted_so_far,
                    still_syntax_node_as_ref(type_node),
                );
            }
            if let Some(pattern_node_in_typed) = maybe_pattern_node_in_typed {
                match &pattern_node_in_typed.value {
                    StillSyntaxPatternUntyped::Ignored => {
                        highlighted_so_far.push(StillSyntaxNode {
                            range: pattern_node_in_typed.range,
                            value: StillSyntaxHighlightKind::KeySymbol,
                        });
                    }
                    StillSyntaxPatternUntyped::Variable(_) => {
                        highlighted_so_far.push(StillSyntaxNode {
                            range: pattern_node_in_typed.range,
                            value: StillSyntaxHighlightKind::Variable,
                        });
                    }
                    StillSyntaxPatternUntyped::Variant {
                        name: name_node,
                        value: maybe_value,
                    } => {
                        highlighted_so_far.push(StillSyntaxNode {
                            range: name_node.range,
                            value: StillSyntaxHighlightKind::Variant,
                        });
                        if let Some(value_node) = maybe_value {
                            still_syntax_highlight_pattern_into(
                                highlighted_so_far,
                                still_syntax_node_unbox(value_node),
                            );
                        }
                    }
                }
            }
        }
        StillSyntaxPattern::Record(fields) => {
            for field in fields {
                highlighted_so_far.push(StillSyntaxNode {
                    range: field.name.range,
                    value: StillSyntaxHighlightKind::Field,
                });
                if let Some(field_value_node) = &field.value {
                    still_syntax_highlight_pattern_into(
                        highlighted_so_far,
                        still_syntax_node_as_ref(field_value_node),
                    );
                }
            }
        }
        StillSyntaxPattern::String {
            content: _,
            quoting_style: _,
        } => {
            highlighted_so_far.push(StillSyntaxNode {
                range: still_syntax_pattern_node.range,
                value: StillSyntaxHighlightKind::String,
            });
        }
    }
}
fn still_syntax_highlight_type_into(
    highlighted_so_far: &mut Vec<StillSyntaxNode<StillSyntaxHighlightKind>>,
    still_syntax_type_node: StillSyntaxNode<&StillSyntaxType>,
) {
    match still_syntax_type_node.value {
        StillSyntaxType::Construct {
            name: name_node,
            arguments,
        } => {
            highlighted_so_far.push(StillSyntaxNode {
                range: name_node.range,
                value: StillSyntaxHighlightKind::Type,
            });
            for argument_node in arguments {
                still_syntax_highlight_type_into(
                    highlighted_so_far,
                    still_syntax_node_as_ref(argument_node),
                );
            }
        }
        StillSyntaxType::Function {
            input: maybe_input,
            arrow_key_symbol_range: maybe_arrow_key_symbol_range,
            output: maybe_output,
        } => {
            if let Some(input) = maybe_input {
                still_syntax_highlight_type_into(
                    highlighted_so_far,
                    still_syntax_node_unbox(input),
                );
            }
            if let Some(arrow_key_symbol_range) = maybe_arrow_key_symbol_range {
                highlighted_so_far.push(StillSyntaxNode {
                    range: *arrow_key_symbol_range,
                    value: StillSyntaxHighlightKind::KeySymbol,
                });
            }
            if let Some(output_node) = maybe_output {
                still_syntax_highlight_type_into(
                    highlighted_so_far,
                    still_syntax_node_unbox(output_node),
                );
            }
        }
        StillSyntaxType::Parenthesized(None) => {}
        StillSyntaxType::Parenthesized(Some(in_parens)) => {
            still_syntax_highlight_type_into(
                highlighted_so_far,
                still_syntax_node_unbox(in_parens),
            );
        }
        StillSyntaxType::Record(fields) => {
            for field in fields {
                highlighted_so_far.push(StillSyntaxNode {
                    range: field.name.range,
                    value: StillSyntaxHighlightKind::Field,
                });
                if let Some(field_value_node) = &field.value {
                    still_syntax_highlight_type_into(
                        highlighted_so_far,
                        still_syntax_node_as_ref(field_value_node),
                    );
                }
            }
        }
        StillSyntaxType::Variable(_) => {
            highlighted_so_far.push(StillSyntaxNode {
                range: still_syntax_type_node.range,
                value: StillSyntaxHighlightKind::TypeVariable,
            });
        }
    }
}

fn still_syntax_highlight_expression_into(
    highlighted_so_far: &mut Vec<StillSyntaxNode<StillSyntaxHighlightKind>>,
    local_bindings: &[StillLocalBinding],
    still_syntax_expression_node: StillSyntaxNode<&StillSyntaxExpression>,
) {
    match still_syntax_expression_node.value {
        StillSyntaxExpression::VariableOrCall {
            variable: variable_node,
            arguments,
        } => {
            if let Some(origin_binding) = local_bindings
                .iter()
                .find(|bind| bind.name == variable_node.value.as_str())
            {
                highlighted_so_far.push(StillSyntaxNode {
                    range: variable_node.range,
                    value: match origin_binding.origin {
                        LocalBindingOrigin::PatternVariable(_) => {
                            StillSyntaxHighlightKind::Variable
                        }
                        LocalBindingOrigin::LetDeclaredVariable { .. } => {
                            StillSyntaxHighlightKind::DeclaredVariable
                        }
                    },
                });
            } else {
                highlighted_so_far.push(StillSyntaxNode {
                    range: variable_node.range,
                    value: StillSyntaxHighlightKind::DeclaredVariable,
                });
            }
            for argument_node in arguments {
                still_syntax_highlight_expression_into(
                    highlighted_so_far,
                    local_bindings,
                    still_syntax_node_as_ref(argument_node),
                );
            }
        }
        StillSyntaxExpression::CaseOf {
            matched: maybe_matched,
            of_keyword_range: maybe_of_keyword_range,
            cases,
        } => {
            highlighted_so_far.push(StillSyntaxNode {
                range: lsp_types::Range {
                    start: still_syntax_expression_node.range.start,
                    end: lsp_position_add_characters(still_syntax_expression_node.range.start, 4),
                },
                value: StillSyntaxHighlightKind::KeySymbol,
            });
            if let Some(matched_node) = maybe_matched {
                still_syntax_highlight_expression_into(
                    highlighted_so_far,
                    local_bindings,
                    still_syntax_node_unbox(matched_node),
                );
            }
            if let &Some(of_keyword_range) = maybe_of_keyword_range {
                highlighted_so_far.push(StillSyntaxNode {
                    range: of_keyword_range,
                    value: StillSyntaxHighlightKind::KeySymbol,
                });
            }
            for case in cases {
                still_syntax_highlight_pattern_into(
                    highlighted_so_far,
                    still_syntax_node_as_ref(&case.pattern),
                );
                if let Some(arrow_key_symbol_range) = case.arrow_key_symbol_range {
                    highlighted_so_far.push(StillSyntaxNode {
                        range: arrow_key_symbol_range,
                        value: StillSyntaxHighlightKind::KeySymbol,
                    });
                }
                if let Some(result_node) = &case.result {
                    let mut local_bindings: Vec<StillLocalBinding> = local_bindings.to_vec();
                    still_syntax_pattern_bindings_into(
                        &mut local_bindings,
                        still_syntax_node_as_ref(&case.pattern),
                    );
                    still_syntax_highlight_expression_into(
                        highlighted_so_far,
                        &local_bindings,
                        still_syntax_node_as_ref(result_node),
                    );
                }
            }
        }
        StillSyntaxExpression::Char(_) => {
            highlighted_so_far.push(StillSyntaxNode {
                range: still_syntax_expression_node.range,
                value: StillSyntaxHighlightKind::String,
            });
        }
        StillSyntaxExpression::Dec(_) => {
            highlighted_so_far.push(StillSyntaxNode {
                range: still_syntax_expression_node.range,
                value: StillSyntaxHighlightKind::Number,
            });
        }
        StillSyntaxExpression::Int { .. } => {
            highlighted_so_far.push(StillSyntaxNode {
                range: still_syntax_expression_node.range,
                value: StillSyntaxHighlightKind::Number,
            });
        }
        StillSyntaxExpression::Lambda {
            parameter: maybe_parameter,
            arrow_key_symbol_range: maybe_arrow_key_symbol_range,
            result: maybe_result,
        } => {
            highlighted_so_far.push(StillSyntaxNode {
                range: lsp_types::Range {
                    start: still_syntax_expression_node.range.start,
                    end: lsp_position_add_characters(still_syntax_expression_node.range.start, 1),
                },
                value: StillSyntaxHighlightKind::KeySymbol,
            });
            if let Some(parameter_node) = maybe_parameter {
                still_syntax_highlight_pattern_into(
                    highlighted_so_far,
                    still_syntax_node_as_ref(parameter_node),
                );
            }
            if let &Some(arrow_key_symbol_range) = maybe_arrow_key_symbol_range {
                highlighted_so_far.push(StillSyntaxNode {
                    range: arrow_key_symbol_range,
                    value: StillSyntaxHighlightKind::KeySymbol,
                });
            }
            if let Some(result_node) = maybe_result {
                let mut local_bindings: Vec<StillLocalBinding> = local_bindings.to_vec();
                if let Some(parameter_node) = maybe_parameter {
                    still_syntax_pattern_bindings_into(
                        &mut local_bindings,
                        still_syntax_node_as_ref(parameter_node),
                    );
                }
                still_syntax_highlight_expression_into(
                    highlighted_so_far,
                    &local_bindings,
                    still_syntax_node_unbox(result_node),
                );
            }
        }
        StillSyntaxExpression::Let {
            declaration: maybe_declaration,
            result: maybe_result,
        } => {
            highlighted_so_far.push(StillSyntaxNode {
                range: lsp_types::Range {
                    start: still_syntax_expression_node.range.start,
                    end: lsp_position_add_characters(still_syntax_expression_node.range.start, 3),
                },
                value: StillSyntaxHighlightKind::KeySymbol,
            });
            let mut local_bindings: Vec<StillLocalBinding> = local_bindings.to_vec();
            if let Some(let_declaration_node) = maybe_declaration {
                still_syntax_let_declaration_introduced_bindings_into(
                    &mut local_bindings,
                    &let_declaration_node.value,
                );
            }
            if let Some(let_declaration_node) = maybe_declaration {
                still_syntax_highlight_let_declaration_into(
                    highlighted_so_far,
                    &local_bindings,
                    still_syntax_node_as_ref(let_declaration_node),
                );
            }
            if let Some(result_node) = maybe_result {
                still_syntax_highlight_expression_into(
                    highlighted_so_far,
                    &local_bindings,
                    still_syntax_node_unbox(result_node),
                );
            }
        }
        StillSyntaxExpression::Vec(elements) => {
            for element_node in elements {
                still_syntax_highlight_expression_into(
                    highlighted_so_far,
                    local_bindings,
                    still_syntax_node_as_ref(element_node),
                );
            }
        }
        StillSyntaxExpression::Parenthesized(None) => {}
        StillSyntaxExpression::Parenthesized(Some(in_parens)) => {
            still_syntax_highlight_expression_into(
                highlighted_so_far,
                local_bindings,
                still_syntax_node_unbox(in_parens),
            );
        }
        StillSyntaxExpression::Typed {
            type_: maybe_type,
            expression: maybe_expression_in_typed,
        } => {
            if let Some(type_node) = maybe_type {
                still_syntax_highlight_type_into(
                    highlighted_so_far,
                    still_syntax_node_as_ref(type_node),
                );
            }
            if let Some(expression_node_in_typed) = maybe_expression_in_typed {
                match &expression_node_in_typed.value {
                    StillSyntaxExpressionUntyped::Variant {
                        name: name_node,
                        value: maybe_value,
                    } => {
                        highlighted_so_far.push(StillSyntaxNode {
                            range: name_node.range,
                            value: StillSyntaxHighlightKind::Variant,
                        });
                        if let Some(value_node) = maybe_value {
                            still_syntax_highlight_expression_into(
                                highlighted_so_far,
                                local_bindings,
                                still_syntax_node_unbox(value_node),
                            );
                        }
                    }
                    StillSyntaxExpressionUntyped::Other(other_expression_in_typed) => {
                        still_syntax_highlight_expression_into(
                            highlighted_so_far,
                            local_bindings,
                            StillSyntaxNode {
                                range: expression_node_in_typed.range,
                                value: other_expression_in_typed,
                            },
                        );
                    }
                }
            }
        }
        StillSyntaxExpression::Record(fields) => {
            for field in fields {
                highlighted_so_far.push(StillSyntaxNode {
                    range: field.name.range,
                    value: StillSyntaxHighlightKind::Field,
                });
                if let Some(value_node) = &field.value {
                    still_syntax_highlight_expression_into(
                        highlighted_so_far,
                        local_bindings,
                        still_syntax_node_as_ref(value_node),
                    );
                }
            }
        }
        StillSyntaxExpression::RecordAccess {
            record: record_node,
            field: maybe_field_name,
        } => {
            still_syntax_highlight_expression_into(
                highlighted_so_far,
                local_bindings,
                still_syntax_node_unbox(record_node),
            );
            highlighted_so_far.push(StillSyntaxNode {
                range: lsp_types::Range {
                    start: record_node.range.end,
                    end: lsp_position_add_characters(record_node.range.end, 1),
                },
                value: StillSyntaxHighlightKind::KeySymbol,
            });
            if let Some(field_name_node) = maybe_field_name {
                highlighted_so_far.push(StillSyntaxNode {
                    range: field_name_node.range,
                    value: StillSyntaxHighlightKind::Field,
                });
            }
        }
        StillSyntaxExpression::RecordUpdate {
            record: maybe_record,
            spread_key_symbol_range,
            fields,
        } => {
            if let Some(record_node) = maybe_record {
                highlighted_so_far.push(StillSyntaxNode {
                    range: record_node.range,
                    value: StillSyntaxHighlightKind::Variable,
                });
            }
            highlighted_so_far.push(StillSyntaxNode {
                range: *spread_key_symbol_range,
                value: StillSyntaxHighlightKind::KeySymbol,
            });
            for field in fields {
                highlighted_so_far.push(StillSyntaxNode {
                    range: field.name.range,
                    value: StillSyntaxHighlightKind::Field,
                });
                if let Some(value_node) = &field.value {
                    still_syntax_highlight_expression_into(
                        highlighted_so_far,
                        local_bindings,
                        still_syntax_node_as_ref(value_node),
                    );
                }
            }
        }
        StillSyntaxExpression::String {
            content,
            quoting_style,
        } => {
            let quote_count: usize = match quoting_style {
                StillSyntaxStringQuotingStyle::SingleQuoted => 1,
                StillSyntaxStringQuotingStyle::TripleQuoted => 3,
            };
            highlighted_so_far.extend(
                still_syntax_highlight_multi_line(
                    StillSyntaxNode {
                        range: still_syntax_expression_node.range,
                        value: content,
                    },
                    quote_count,
                    quote_count,
                )
                .map(|range| StillSyntaxNode {
                    range: range,
                    value: StillSyntaxHighlightKind::String,
                }),
            );
        }
    }
}

fn still_syntax_highlight_let_declaration_into(
    highlighted_so_far: &mut Vec<StillSyntaxNode<StillSyntaxHighlightKind>>,
    local_bindings: &[StillLocalBinding],
    still_syntax_let_declaration_node: StillSyntaxNode<&StillSyntaxLetDeclaration>,
) {
    match still_syntax_let_declaration_node.value {
        StillSyntaxLetDeclaration::Destructuring {
            pattern: destructuring_pattern_node,
            equals_key_symbol_range: maybe_equals_key_symbol_range,
            expression: maybe_destructured_expression,
        } => {
            still_syntax_highlight_pattern_into(
                highlighted_so_far,
                still_syntax_node_as_ref(destructuring_pattern_node),
            );
            if let &Some(equals_key_symbol_range) = maybe_equals_key_symbol_range {
                highlighted_so_far.push(StillSyntaxNode {
                    range: equals_key_symbol_range,
                    value: StillSyntaxHighlightKind::KeySymbol,
                });
            }
            if let Some(destructured_expression_node) = maybe_destructured_expression {
                still_syntax_highlight_expression_into(
                    highlighted_so_far,
                    local_bindings,
                    still_syntax_node_unbox(destructured_expression_node),
                );
            }
        }
        StillSyntaxLetDeclaration::VariableDeclaration {
            start_name: start_name_node,
            result: maybe_result,
        } => {
            highlighted_so_far.push(StillSyntaxNode {
                range: start_name_node.range,
                value: StillSyntaxHighlightKind::DeclaredVariable,
            });
            if let Some(result_node) = maybe_result {
                still_syntax_highlight_expression_into(
                    highlighted_so_far,
                    local_bindings,
                    still_syntax_node_unbox(result_node),
                );
            }
        }
    }
}

// //
struct ParseState<'a> {
    source: &'a str,
    offset_utf8: usize,
    position: lsp_types::Position,
    indent: u16,
    lower_indents_stack: Vec<u16>,
    comments: Vec<StillSyntaxNode<Box<str>>>,
}

fn parse_state_push_indent(state: &mut ParseState, new_indent: u16) {
    state.lower_indents_stack.push(state.indent);
    state.indent = new_indent;
}
fn parse_state_pop_indent(state: &mut ParseState) {
    state.indent = state.lower_indents_stack.pop().unwrap_or(0);
}

fn str_starts_with_linebreak(str: &str) -> bool {
    // \r allowed because both \r and \r\n are counted as linebreak
    // see EOL in https://microsoft.github.io/language-server-protocol/specifications/lsp/3.17/specification/#textDocuments
    str.starts_with("\n") || str.starts_with("\r")
}
fn parse_linebreak(state: &mut ParseState) -> bool {
    // see EOL in https://microsoft.github.io/language-server-protocol/specifications/lsp/3.17/specification/#textDocuments
    if state.source[state.offset_utf8..].starts_with("\n") {
        state.offset_utf8 += 1;
        state.position.line += 1;
        state.position.character = 0;
        true
    } else if state.source[state.offset_utf8..].starts_with("\r\n") {
        state.offset_utf8 += 2;
        state.position.line += 1;
        state.position.character = 0;
        true
    } else if state.source[state.offset_utf8..].starts_with("\r") {
        state.offset_utf8 += 1;
        state.position.line += 1;
        state.position.character = 0;
        true
    } else {
        false
    }
}
fn parse_linebreak_as_str<'a>(state: &mut ParseState<'a>) -> Option<&'a str> {
    // see EOL in https://microsoft.github.io/language-server-protocol/specifications/lsp/3.17/specification/#textDocuments
    if state.source[state.offset_utf8..].starts_with("\n") {
        state.offset_utf8 += 1;
        state.position.line += 1;
        state.position.character = 0;
        Some("\n")
    } else if state.source[state.offset_utf8..].starts_with("\r\n") {
        state.offset_utf8 += 2;
        state.position.line += 1;
        state.position.character = 0;
        Some("\r\n")
    } else if state.source[state.offset_utf8..].starts_with("\r") {
        state.offset_utf8 += 1;
        state.position.line += 1;
        state.position.character = 0;
        Some("\r")
    } else {
        None
    }
}
/// prefer using after `parse_line_break` or similar failed
fn parse_any_guaranteed_non_linebreak_char(state: &mut ParseState) -> bool {
    match state.source[state.offset_utf8..].chars().next() {
        None => false,
        Some(parsed_char) => {
            state.offset_utf8 += parsed_char.len_utf8();
            state.position.character += parsed_char.len_utf16() as u32;
            true
        }
    }
}
/// prefer using after `parse_line_break` or similar failed
fn parse_any_guaranteed_non_linebreak_char_as_char(state: &mut ParseState) -> Option<char> {
    match state.source[state.offset_utf8..].chars().next() {
        None => None,
        Some(parsed_char) => {
            state.offset_utf8 += parsed_char.len_utf8();
            state.position.character += parsed_char.len_utf16() as u32;
            Some(parsed_char)
        }
    }
}
/// symbol cannot be non-utf8 characters or \n
fn parse_char_symbol_as_char(state: &mut ParseState, symbol: char) -> Option<char> {
    if state.source[state.offset_utf8..].starts_with(symbol) {
        state.offset_utf8 += symbol.len_utf8();
        state.position.character += symbol.len_utf16() as u32;
        Some(symbol)
    } else {
        None
    }
}
/// symbol cannot contain non-utf8 characters or \n
fn parse_symbol(state: &mut ParseState, symbol: &str) -> bool {
    if state.source[state.offset_utf8..].starts_with(symbol) {
        state.offset_utf8 += symbol.len();
        state.position.character += symbol.len() as u32;
        true
    } else {
        false
    }
}
/// symbol cannot contain non-utf8 characters or \n
fn parse_symbol_as<A>(state: &mut ParseState, symbol: &'static str, result: A) -> Option<A> {
    if parse_symbol(state, symbol) {
        Some(result)
    } else {
        None
    }
}
/// symbol cannot contain non-utf8 characters or \n
fn parse_symbol_as_range(state: &mut ParseState, symbol: &str) -> Option<lsp_types::Range> {
    let start_position: lsp_types::Position = state.position;
    if parse_symbol(state, symbol) {
        Some(lsp_types::Range {
            start: start_position,
            end: state.position,
        })
    } else {
        None
    }
}
/// given condition must not succeed on linebreak
fn parse_same_line_while(state: &mut ParseState, char_is_valid: impl Fn(char) -> bool) {
    let consumed_chars_iterator = state.source[state.offset_utf8..]
        .chars()
        .take_while(|&c| char_is_valid(c));
    let consumed_length_utf8: usize = consumed_chars_iterator.clone().map(char::len_utf8).sum();
    let consumed_length_utf16: usize = consumed_chars_iterator.clone().map(char::len_utf16).sum();
    state.offset_utf8 += consumed_length_utf8;
    state.position.character += consumed_length_utf16 as u32;
}
fn parse_before_next_linebreak(state: &mut ParseState) {
    parse_same_line_while(state, |c| c != '\r' && c != '\n');
}
/// given condition must not succeed on linebreak
fn parse_same_line_char_if(state: &mut ParseState, char_is_valid: impl Fn(char) -> bool) -> bool {
    if let Some(next_char) = state.source[state.offset_utf8..].chars().next()
        && char_is_valid(next_char)
    {
        state.offset_utf8 += next_char.len_utf8();
        state.position.character += next_char.len_utf16() as u32;
        true
    } else {
        false
    }
}
fn parse_unsigned_integer_base10(state: &mut ParseState) -> bool {
    if parse_symbol(state, "0") {
        true
    } else if parse_same_line_char_if(state, |c| ('1'..='9').contains(&c)) {
        parse_same_line_while(state, |c| c.is_ascii_digit());
        true
    } else {
        false
    }
}

/// a valid still symbol that must be followed by a character that could not be part of an still identifier
fn parse_still_keyword_as_range(state: &mut ParseState, symbol: &str) -> Option<lsp_types::Range> {
    if state.source[state.offset_utf8..].starts_with(symbol)
        && !(state.source[(state.offset_utf8 + symbol.len())..]
            .starts_with(|c: char| c.is_ascii_alphanumeric() || c == '-'))
    {
        let start_position: lsp_types::Position = state.position;
        state.offset_utf8 += symbol.len();
        state.position.character += symbol.len() as u32;
        Some(lsp_types::Range {
            start: start_position,
            end: state.position,
        })
    } else {
        None
    }
}

fn parse_still_whitespace_and_comments(state: &mut ParseState) {
    while parse_linebreak(state)
        || parse_same_line_char_if(state, char::is_whitespace)
        || parse_still_comment(state)
    {}
}
fn parse_still_comment(state: &mut ParseState) -> bool {
    let position_before: lsp_types::Position = state.position;
    if !parse_symbol(state, "#") {
        return false;
    }
    let content: &str = state.source[state.offset_utf8..]
        .lines()
        .next()
        .unwrap_or("");
    state.offset_utf8 += content.len();
    state.position.character += content.encode_utf16().count() as u32;
    let full_range: lsp_types::Range = lsp_types::Range {
        start: position_before,
        end: state.position,
    };
    state.comments.push(StillSyntaxNode {
        range: full_range,
        value: Box::from(content),
    });
    true
}
fn parse_still_documentation_comment_block_str<'a>(state: &mut ParseState<'a>) -> Option<&'a str> {
    if !parse_symbol(state, "(#") {
        return None;
    }
    let content_start_offset_utf8: usize = state.offset_utf8;
    let mut nesting_level: u32 = 1;
    'until_fully_unnested: loop {
        if parse_linebreak(state) {
        } else if parse_symbol(state, "(#") {
            nesting_level += 1;
        } else if parse_symbol(state, "#)") {
            if nesting_level <= 1 {
                break 'until_fully_unnested;
            }
            nesting_level -= 1;
        } else if parse_any_guaranteed_non_linebreak_char(state) {
        } else {
            // end of source
            break 'until_fully_unnested;
        }
    }
    let content_including_closing: &str =
        &state.source[content_start_offset_utf8..state.offset_utf8];
    Some(
        content_including_closing
            .strip_suffix("#)")
            .unwrap_or(content_including_closing),
    )
}
fn parse_still_documentation_comment_block_node(
    state: &mut ParseState,
) -> Option<StillSyntaxNode<Box<str>>> {
    let start_position: lsp_types::Position = state.position;
    let content: &str = parse_still_documentation_comment_block_str(state)?;
    Some(StillSyntaxNode {
        range: lsp_types::Range {
            start: start_position,
            end: state.position,
        },
        value: Box::from(content),
    })
}
fn parse_still_lowercase_name(state: &mut ParseState) -> Option<StillName> {
    let mut chars_from_offset: std::str::Chars = state.source[state.offset_utf8..].chars();
    if let Some(first_char) = chars_from_offset.next()
        && first_char.is_ascii_lowercase()
    {
        let parsed_length: usize = first_char.len_utf8()
            + chars_from_offset
                .take_while(|&c| c.is_ascii_alphanumeric() || c == '-')
                .map(char::len_utf8)
                .sum::<usize>();
        let end_offset_utf8: usize = state.offset_utf8 + parsed_length;
        let parsed_str: &str = &state.source[state.offset_utf8..end_offset_utf8];
        state.offset_utf8 = end_offset_utf8;
        state.position.character += parsed_str.encode_utf16().count() as u32;
        Some(StillName::from(parsed_str))
    } else {
        None
    }
}
fn parse_still_lowercase_name_node(state: &mut ParseState) -> Option<StillSyntaxNode<StillName>> {
    let start_position: lsp_types::Position = state.position;
    parse_still_lowercase_name(state).map(|name| StillSyntaxNode {
        range: lsp_types::Range {
            start: start_position,
            end: state.position,
        },
        value: name,
    })
}

fn parse_still_uppercase_as_name(state: &mut ParseState) -> Option<StillName> {
    let mut chars_from_offset = state.source[state.offset_utf8..].chars();
    if let Some(first_char) = chars_from_offset.next()
        && first_char.is_ascii_uppercase()
    {
        let parsed_length: usize = first_char.len_utf8()
            + chars_from_offset
                .take_while(|&c| c.is_ascii_alphanumeric() || c == '-')
                .map(char::len_utf8)
                .sum::<usize>();
        let end_offset_utf8: usize = state.offset_utf8 + parsed_length;
        let parsed_str: &str = &state.source[state.offset_utf8..end_offset_utf8];
        state.offset_utf8 = end_offset_utf8;
        state.position.character += parsed_str.encode_utf16().count() as u32;
        Some(StillName::from(parsed_str))
    } else {
        None
    }
}

fn parse_still_uppercase_name_node(state: &mut ParseState) -> Option<StillSyntaxNode<StillName>> {
    let start_position: lsp_types::Position = state.position;
    parse_still_uppercase_as_name(state).map(|name| StillSyntaxNode {
        range: lsp_types::Range {
            start: start_position,
            end: state.position,
        },
        value: name,
    })
}

fn parse_still_syntax_type_space_separated_node(
    state: &mut ParseState,
) -> Option<StillSyntaxNode<StillSyntaxType>> {
    let backslash_range: lsp_types::Range = parse_symbol_as_range(state, "\\")?;
    let maybe_input_type_node: Option<StillSyntaxNode<StillSyntaxType>> =
        parse_still_syntax_type_not_function_node(state);
    parse_still_whitespace_and_comments(state);
    let maybe_arrow_key_symbol_range = parse_symbol_as_range(state, "->");
    parse_still_whitespace_and_comments(state);
    let maybe_output_type: Option<StillSyntaxNode<StillSyntaxType>> =
        if state.position.character > u32::from(state.indent) {
            parse_still_syntax_type_space_separated_node(state)
        } else {
            None
        };
    Some(StillSyntaxNode {
        range: lsp_types::Range {
            start: maybe_input_type_node
                .as_ref()
                .map(|n| n.range.start)
                .unwrap_or(backslash_range.start),
            end: match &maybe_output_type {
                None => maybe_arrow_key_symbol_range
                    .map(|r| r.end)
                    .or_else(|| maybe_input_type_node.as_ref().map(|n| n.range.end))
                    .unwrap_or(backslash_range.end),
                Some(output_type_node) => output_type_node.range.end,
            },
        },
        value: StillSyntaxType::Function {
            input: maybe_input_type_node.map(still_syntax_node_box),
            arrow_key_symbol_range: maybe_arrow_key_symbol_range,
            output: maybe_output_type.map(still_syntax_node_box),
        },
    })
}
fn parse_still_syntax_type_not_function_node(
    state: &mut ParseState,
) -> Option<StillSyntaxNode<StillSyntaxType>> {
    if state.position.character <= u32::from(state.indent) {
        return None;
    }
    parse_still_syntax_type_construct_node(state).or_else(|| {
        let start_position: lsp_types::Position = state.position;
        parse_still_lowercase_name(state)
            .map(StillSyntaxType::Variable)
            .or_else(|| parse_still_syntax_type_parenthesized(state))
            .or_else(|| parse_still_syntax_type_record(state))
            .map(|type_| StillSyntaxNode {
                range: lsp_types::Range {
                    start: start_position,
                    end: state.position,
                },
                value: type_,
            })
    })
}
fn parse_still_syntax_type_not_space_separated_node(
    state: &mut ParseState,
) -> Option<StillSyntaxNode<StillSyntaxType>> {
    let start_position: lsp_types::Position = state.position;
    parse_still_syntax_type_not_space_separated(state).map(|type_| StillSyntaxNode {
        range: lsp_types::Range {
            start: start_position,
            end: state.position,
        },
        value: type_,
    })
}
fn parse_still_syntax_type_not_space_separated(state: &mut ParseState) -> Option<StillSyntaxType> {
    if state.position.character <= u32::from(state.indent) {
        return None;
    }
    parse_still_lowercase_name(state)
        .map(StillSyntaxType::Variable)
        .or_else(|| parse_still_syntax_type_parenthesized(state))
        .or_else(|| {
            parse_still_qualified_uppercase_variable_node(state).map(|variable_node| {
                StillSyntaxType::Construct {
                    name: variable_node,
                    arguments: vec![],
                }
            })
        })
        .or_else(|| parse_still_syntax_type_record(state))
}
fn parse_still_syntax_type_record(state: &mut ParseState) -> Option<StillSyntaxType> {
    if !parse_symbol(state, "{") {
        return None;
    }
    parse_still_whitespace_and_comments(state);
    while parse_symbol(state, ",") {
        parse_still_whitespace_and_comments(state);
    }
    let maybe_start_name: Option<StillSyntaxNode<StillName>> =
        parse_still_lowercase_name_node(state);
    parse_still_whitespace_and_comments(state);
    match maybe_start_name {
        None => {
            let _: bool = parse_symbol(state, "}");
            Some(StillSyntaxType::Record(vec![]))
        }
        Some(field0_name_node) => {
            let maybe_field0_value: Option<StillSyntaxNode<StillSyntaxType>> =
                parse_still_syntax_type_space_separated_node(state);
            parse_still_whitespace_and_comments(state);
            while parse_symbol(state, ",") {
                parse_still_whitespace_and_comments(state);
            }
            let mut fields: Vec<StillSyntaxTypeField> = vec![StillSyntaxTypeField {
                name: field0_name_node,
                value: maybe_field0_value,
            }];
            while let Some(field) = parse_still_syntax_type_field(state) {
                fields.push(field);
                parse_still_whitespace_and_comments(state);
                while parse_symbol(state, ",") {
                    parse_still_whitespace_and_comments(state);
                }
            }
            let _: bool = parse_symbol(state, "}");
            Some(StillSyntaxType::Record(fields))
        }
    }
}
fn parse_still_syntax_type_field(state: &mut ParseState) -> Option<StillSyntaxTypeField> {
    if state.position.character <= u32::from(state.indent) {
        return None;
    }
    let maybe_name: StillSyntaxNode<StillName> = parse_still_lowercase_name_node(state)?;
    parse_still_whitespace_and_comments(state);
    let maybe_value: Option<StillSyntaxNode<StillSyntaxType>> =
        parse_still_syntax_type_space_separated_node(state);
    Some(StillSyntaxTypeField {
        name: maybe_name,
        value: maybe_value,
    })
}
fn parse_still_syntax_type_construct_node(
    state: &mut ParseState,
) -> Option<StillSyntaxNode<StillSyntaxType>> {
    let variable_node: StillSyntaxNode<StillName> =
        parse_still_qualified_uppercase_variable_node(state)?;
    parse_still_whitespace_and_comments(state);
    let mut arguments: Vec<StillSyntaxNode<StillSyntaxType>> = Vec::new();
    let mut construct_end_position: lsp_types::Position = variable_node.range.end;
    while let Some(argument_node) = parse_still_syntax_type_not_space_separated_node(state) {
        construct_end_position = argument_node.range.end;
        arguments.push(argument_node);
        parse_still_whitespace_and_comments(state);
    }
    Some(StillSyntaxNode {
        range: lsp_types::Range {
            start: variable_node.range.start,
            end: construct_end_position,
        },
        value: StillSyntaxType::Construct {
            name: variable_node,
            arguments: arguments,
        },
    })
}
/// TODO inline
fn parse_still_qualified_uppercase_variable_node(
    state: &mut ParseState,
) -> Option<StillSyntaxNode<StillName>> {
    parse_still_uppercase_name_node(state)
}
fn parse_still_syntax_type_parenthesized(state: &mut ParseState) -> Option<StillSyntaxType> {
    if !parse_symbol(state, "(") {
        return None;
    }
    parse_still_whitespace_and_comments(state);
    let maybe_in_parens_0: Option<StillSyntaxNode<StillSyntaxType>> =
        parse_still_syntax_type_space_separated_node(state);
    parse_still_whitespace_and_comments(state);
    let _ = parse_symbol(state, ")");
    Some(StillSyntaxType::Parenthesized(
        maybe_in_parens_0.map(still_syntax_node_box),
    ))
}

fn parse_still_syntax_pattern_node(
    state: &mut ParseState,
) -> Option<StillSyntaxNode<StillSyntaxPattern>> {
    if state.position.character <= u32::from(state.indent) {
        return None;
    }
    let start_position: lsp_types::Position = state.position;
    parse_still_char(state)
        .map(StillSyntaxPattern::Char)
        .or_else(|| parse_still_syntax_pattern_string(state))
        .or_else(|| parse_still_syntax_pattern_record(state))
        .or_else(|| parse_still_syntax_pattern_int(state))
        .map(|pattern| StillSyntaxNode {
            range: lsp_types::Range {
                start: start_position,
                end: state.position,
            },
            value: pattern,
        })
        .or_else(|| parse_still_syntax_pattern_typed(state))
}
fn parse_still_syntax_pattern_record(state: &mut ParseState) -> Option<StillSyntaxPattern> {
    if !parse_symbol(state, "{") {
        return None;
    }
    parse_still_whitespace_and_comments(state);
    while parse_symbol(state, ",") {
        parse_still_whitespace_and_comments(state);
    }
    let mut fields: Vec<StillSyntaxPatternField> = Vec::new();
    while let Some(field_name_node) = if state.position.character <= u32::from(state.indent) {
        None
    } else {
        parse_still_lowercase_name_node(state)
    } {
        parse_still_whitespace_and_comments(state);
        let maybe_value: Option<StillSyntaxNode<StillSyntaxPattern>> =
            parse_still_syntax_pattern_node(state);
        fields.push(StillSyntaxPatternField {
            name: field_name_node,
            value: maybe_value,
        });
        parse_still_whitespace_and_comments(state);
        while parse_symbol(state, ",") {
            parse_still_whitespace_and_comments(state);
        }
    }
    let _: bool = parse_symbol(state, "}");
    Some(StillSyntaxPattern::Record(fields))
}
fn parse_still_syntax_pattern_typed(
    state: &mut ParseState,
) -> Option<StillSyntaxNode<StillSyntaxPattern>> {
    let start_position: lsp_types::Position = state.position;
    if !parse_symbol(state, ":") {
        return None;
    }
    parse_still_whitespace_and_comments(state);
    let maybe_type: Option<StillSyntaxNode<StillSyntaxType>> =
        parse_still_syntax_type_space_separated_node(state);
    parse_still_whitespace_and_comments(state);
    let closing_colon_range: Option<lsp_types::Range> = parse_symbol_as_range(state, ":");
    let maybe_pattern: Option<StillSyntaxNode<StillSyntaxPatternUntyped>> =
        parse_still_syntax_pattern_untyped_node(state);
    Some(StillSyntaxNode {
        range: lsp_types::Range {
            start: start_position,
            end: maybe_pattern
                .as_ref()
                .map(|n| n.range.end)
                .or_else(|| closing_colon_range.map(|r| r.end))
                .or_else(|| maybe_type.as_ref().map(|n| n.range.end))
                .unwrap_or_else(|| lsp_position_add_characters(start_position, 1)),
        },
        value: StillSyntaxPattern::Typed {
            type_: maybe_type,
            pattern: maybe_pattern,
        },
    })
}
fn parse_still_syntax_pattern_untyped_node(
    state: &mut ParseState,
) -> Option<StillSyntaxNode<StillSyntaxPatternUntyped>> {
    parse_symbol_as_range(state, "_")
        .map(|range| StillSyntaxNode {
            range: range,
            value: StillSyntaxPatternUntyped::Ignored,
        })
        .or_else(|| {
            parse_still_lowercase_name_node(state)
                .map(|n| still_syntax_node_map(n, StillSyntaxPatternUntyped::Variable))
        })
        .or_else(|| parse_still_syntax_pattern_variant_node(state))
}
fn parse_still_syntax_pattern_variant_node(
    state: &mut ParseState,
) -> Option<StillSyntaxNode<StillSyntaxPatternUntyped>> {
    let variable_node: StillSyntaxNode<StillName> =
        parse_still_qualified_uppercase_variable_node(state)?;
    parse_still_whitespace_and_comments(state);
    let maybe_value: Option<StillSyntaxNode<StillSyntaxPattern>> =
        parse_still_syntax_pattern_node(state);
    Some(StillSyntaxNode {
        range: lsp_types::Range {
            start: variable_node.range.start,
            end: match &maybe_value {
                None => variable_node.range.end,
                Some(value_node) => value_node.range.end,
            },
        },
        value: StillSyntaxPatternUntyped::Variant {
            name: variable_node,
            value: maybe_value.map(still_syntax_node_box),
        },
    })
}
fn parse_still_syntax_pattern_string(state: &mut ParseState) -> Option<StillSyntaxPattern> {
    parse_still_string_triple_quoted(state)
        .map(|content| StillSyntaxPattern::String {
            content: content,
            quoting_style: StillSyntaxStringQuotingStyle::TripleQuoted,
        })
        .or_else(|| {
            parse_still_string_single_quoted(state).map(|content| StillSyntaxPattern::String {
                content: content,
                quoting_style: StillSyntaxStringQuotingStyle::SingleQuoted,
            })
        })
}

fn parse_still_syntax_pattern_int(state: &mut ParseState) -> Option<StillSyntaxPattern> {
    parse_still_unsigned_integer_base10_as_i64(state)
        .map(|value| StillSyntaxPattern::Int { value: value })
}
fn parse_still_unsigned_integer_base10_as_i64(
    state: &mut ParseState,
) -> Option<Result<i64, Box<str>>> {
    let start_offset_utf8: usize = state.offset_utf8;
    if parse_unsigned_integer_base10(state) {
        let decimal_str: &str = &state.source[start_offset_utf8..state.offset_utf8];
        Some(str::parse::<i64>(decimal_str).map_err(|_| Box::from(decimal_str)))
    } else {
        None
    }
}
fn parse_still_syntax_expression_number(state: &mut ParseState) -> Option<StillSyntaxExpression> {
    let start_offset_utf8: usize = state.offset_utf8;
    if !(parse_unsigned_integer_base10(state)
        || parse_symbol(state, "-")
        || parse_symbol(state, "+"))
    {
        return None;
    }
    let has_decimal_point: bool = parse_symbol(state, ".");
    if has_decimal_point {
        parse_same_line_while(state, |c| c.is_ascii_digit());
    }
    let full_chomped_str: &str = &state.source[start_offset_utf8..state.offset_utf8];
    Some(if has_decimal_point {
        StillSyntaxExpression::Dec(
            str::parse::<f64>(full_chomped_str).map_err(|_| Box::from(full_chomped_str)),
        )
    } else {
        StillSyntaxExpression::Int {
            value: str::parse::<i64>(full_chomped_str).map_err(|_| Box::from(full_chomped_str)),
        }
    })
}
fn parse_still_char(state: &mut ParseState) -> Option<Option<char>> {
    if !parse_symbol(state, "'") {
        return None;
    }
    let result: Option<char> = parse_still_text_content_char(state);
    let _: bool = parse_symbol(state, "'");
    Some(result)
}
/// commits after a single quote, so check for triple quoted beforehand
fn parse_still_string_single_quoted(state: &mut ParseState) -> Option<String> {
    if !parse_symbol(state, "\"") {
        return None;
    }
    let mut result: String = String::new();
    while !(parse_symbol(state, "\"")
        || str_starts_with_linebreak(&state.source[state.offset_utf8..]))
    {
        match parse_still_text_content_char(state) {
            Some(next_content_char) => {
                result.push(next_content_char);
            }
            None => match parse_any_guaranteed_non_linebreak_char_as_char(state) {
                Some(next_content_char) => {
                    result.push(next_content_char);
                }
                None => return Some(result),
            },
        }
    }
    Some(result)
}
fn parse_still_string_triple_quoted(state: &mut ParseState) -> Option<String> {
    if !parse_symbol(state, "\"\"\"") {
        return None;
    }
    let mut result: String = String::new();
    while !parse_symbol(state, "\"\"\"") {
        match parse_linebreak_as_str(state) {
            Some(linebreak) => result.push_str(linebreak),
            None => match parse_char_symbol_as_char(state, '\"')
                .or_else(|| parse_still_text_content_char(state))
            {
                Some(next_content_char) => {
                    result.push(next_content_char);
                }
                None => match parse_any_guaranteed_non_linebreak_char_as_char(state) {
                    Some(next_content_char) => {
                        result.push(next_content_char);
                    }
                    None => return Some(result),
                },
            },
        }
    }
    Some(result)
}
fn parse_still_text_content_char(state: &mut ParseState) -> Option<char> {
    parse_symbol_as(state, "\\\\", '\\')
        .or_else(|| parse_symbol_as(state, "\\'", '\''))
        .or_else(|| parse_symbol_as(state, "\\\n", '\n'))
        .or_else(|| parse_symbol_as(state, "\\\r", '\r'))
        .or_else(|| parse_symbol_as(state, "\\\t", '\t'))
        .or_else(|| parse_symbol_as(state, "\\\"", '"'))
        .or_else(|| {
            let start_offset_utf8: usize = state.offset_utf8;
            let start_position: lsp_types::Position = state.position;
            let reset_parse_state = |progressed_state: &mut ParseState| {
                progressed_state.offset_utf8 = start_offset_utf8;
                progressed_state.position = start_position;
            };
            if !parse_symbol(state, "\\u{") {
                return None;
            }
            let unicode_hex_start_offset_utf8: usize = state.offset_utf8;
            parse_same_line_while(state, |c| c.is_ascii_hexdigit());
            let unicode_hex_str: &str =
                &state.source[unicode_hex_start_offset_utf8..state.offset_utf8];
            let _: bool = parse_symbol(state, "}");
            let Ok(first_utf16_code) = u16::from_str_radix(unicode_hex_str, 16) else {
                reset_parse_state(state);
                return None;
            };
            match char::from_u32(u32::from(first_utf16_code)) {
                Some(char) => Some(char),
                None => {
                    if !parse_symbol(state, "\\u{") {
                        reset_parse_state(state);
                        return None;
                    }
                    let second_unicode_hex_start_offset_utf8: usize = state.offset_utf8;
                    parse_same_line_while(state, |c| c.is_ascii_hexdigit());
                    let second_unicode_hex_str: &str =
                        &state.source[second_unicode_hex_start_offset_utf8..state.offset_utf8];
                    let _: bool = parse_symbol(state, "}");
                    let Ok(second_utf16_code) = u16::from_str_radix(second_unicode_hex_str, 16)
                    else {
                        reset_parse_state(state);
                        return None;
                    };
                    char::decode_utf16([first_utf16_code, second_utf16_code])
                        .find_map(Result::ok)
                        .or_else(|| {
                            reset_parse_state(state);
                            None
                        })
                }
            }
        })
        .or_else(|| {
            if str_starts_with_linebreak(&state.source[state.offset_utf8..]) {
                None
            } else {
                match state.source[state.offset_utf8..].chars().next() {
                    None => None,
                    Some(plain_char) => {
                        state.offset_utf8 += plain_char.len_utf8();
                        state.position.character += plain_char.len_utf16() as u32;
                        Some(plain_char)
                    }
                }
            }
        })
}

fn parse_still_syntax_expression_space_separated_node(
    state: &mut ParseState,
) -> Option<StillSyntaxNode<StillSyntaxExpression>> {
    if state.position.character <= u32::from(state.indent) {
        return None;
    }
    parse_still_syntax_expression_typed(state)
        .or_else(|| parse_still_syntax_expression_case_of(state))
        .or_else(|| parse_still_syntax_expression_let_in(state))
        .or_else(|| parse_still_syntax_expression_lambda(state))
        .or_else(|| parse_still_syntax_expression_call(state))
        .or_else(|| parse_still_syntax_expression_not_space_separated_node(state))
}
fn parse_still_syntax_expression_untyped_node(
    state: &mut ParseState,
) -> Option<StillSyntaxNode<StillSyntaxExpressionUntyped>> {
    if state.position.character <= u32::from(state.indent) {
        return None;
    }
    parse_still_syntax_expression_variant_node(state).or_else(|| {
        parse_still_syntax_expression_space_separated_node(state).map(|n| StillSyntaxNode {
            range: n.range,
            value: StillSyntaxExpressionUntyped::Other(Box::new(n.value)),
        })
    })
}
fn parse_still_syntax_expression_typed(
    state: &mut ParseState,
) -> Option<StillSyntaxNode<StillSyntaxExpression>> {
    let start_position: lsp_types::Position = state.position;
    if !parse_symbol(state, ":") {
        return None;
    }
    parse_still_whitespace_and_comments(state);
    let maybe_type: Option<StillSyntaxNode<StillSyntaxType>> =
        parse_still_syntax_type_space_separated_node(state);
    parse_still_whitespace_and_comments(state);
    let closing_colon_range: Option<lsp_types::Range> = parse_symbol_as_range(state, ":");
    let maybe_expression: Option<StillSyntaxNode<StillSyntaxExpressionUntyped>> =
        parse_still_syntax_expression_untyped_node(state);
    Some(StillSyntaxNode {
        range: lsp_types::Range {
            start: start_position,
            end: maybe_expression
                .as_ref()
                .map(|n| n.range.end)
                .or_else(|| closing_colon_range.map(|r| r.end))
                .or_else(|| maybe_type.as_ref().map(|n| n.range.end))
                .unwrap_or_else(|| lsp_position_add_characters(start_position, 1)),
        },
        value: StillSyntaxExpression::Typed {
            type_: maybe_type,
            expression: maybe_expression,
        },
    })
}
fn parse_still_syntax_expression_call(
    state: &mut ParseState,
) -> Option<StillSyntaxNode<StillSyntaxExpression>> {
    let variable_node: StillSyntaxNode<StillName> =
        parse_still_syntax_expression_variable_standalone(state)?;
    parse_still_whitespace_and_comments(state);
    let mut arguments: Vec<StillSyntaxNode<StillSyntaxExpression>> = Vec::new();
    let mut call_end_position: lsp_types::Position = variable_node.range.end;
    'parsing_arguments: loop {
        if state.position.character <= u32::from(state.indent) {
            break 'parsing_arguments;
        }
        match parse_still_syntax_expression_not_space_separated_node(state) {
            None => {
                break 'parsing_arguments;
            }
            Some(argument_node) => {
                call_end_position = argument_node.range.end;
                arguments.push(argument_node);
                parse_still_whitespace_and_comments(state);
            }
        }
    }
    Some(StillSyntaxNode {
        range: lsp_types::Range {
            start: variable_node.range.start,
            end: call_end_position,
        },
        value: StillSyntaxExpression::VariableOrCall {
            variable: variable_node,
            arguments,
        },
    })
}
fn parse_still_syntax_expression_variant_node(
    state: &mut ParseState,
) -> Option<StillSyntaxNode<StillSyntaxExpressionUntyped>> {
    let name_node: StillSyntaxNode<StillName> = parse_still_uppercase_name_node(state)?;
    parse_still_whitespace_and_comments(state);
    let maybe_value: Option<StillSyntaxNode<StillSyntaxExpression>> = {
        if state.position.character <= u32::from(state.indent) {
            None
        } else {
            parse_still_syntax_expression_not_space_separated_node(state)
        }
    };
    Some(StillSyntaxNode {
        range: lsp_types::Range {
            start: name_node.range.start,
            end: maybe_value
                .as_ref()
                .map(|n| n.range.end)
                .unwrap_or(name_node.range.end),
        },
        value: StillSyntaxExpressionUntyped::Variant {
            name: name_node,
            value: maybe_value.map(still_syntax_node_box),
        },
    })
}
fn parse_still_syntax_expression_not_space_separated_node(
    state: &mut ParseState,
) -> Option<StillSyntaxNode<StillSyntaxExpression>> {
    let start_position: lsp_types::Position = state.position;
    let start_expression: StillSyntaxExpression = parse_still_syntax_expression_string(state)
        .or_else(|| parse_still_syntax_expression_list(state))
        .or_else(|| parse_still_syntax_expression_parenthesized(state))
        .or_else(|| parse_still_syntax_expression_variable(state))
        .or_else(|| parse_still_syntax_expression_record_or_record_update(state))
        .or_else(|| parse_still_syntax_expression_number(state))
        .or_else(|| parse_still_char(state).map(StillSyntaxExpression::Char))?;
    let mut result_node: StillSyntaxNode<StillSyntaxExpression> = StillSyntaxNode {
        range: lsp_types::Range {
            start: start_position,
            end: state.position,
        },
        value: start_expression,
    };
    while parse_symbol(state, ".") {
        let maybe_field_name: Option<StillSyntaxNode<StillName>> =
            parse_still_lowercase_name_node(state);
        result_node = StillSyntaxNode {
            range: lsp_types::Range {
                start: start_position,
                end: state.position,
            },
            value: StillSyntaxExpression::RecordAccess {
                record: still_syntax_node_box(result_node),
                field: maybe_field_name,
            },
        }
    }
    Some(result_node)
}
fn str_starts_with_keyword(source: &str, keyword: &'static str) -> bool {
    source.starts_with(keyword)
        && source
            .chars()
            .skip(keyword.len())
            .next()
            .is_some_and(|c| c != '-' && !c.is_ascii_alphanumeric())
}
fn parse_still_syntax_expression_variable_standalone(
    state: &mut ParseState,
) -> Option<StillSyntaxNode<StillName>> {
    // can be optimized by e.g. adding a non-state-mutating parse_still_lowercase_as_string
    // that checks for keywords on successful chomp and returns None only then (and if no keyword, mutate the state)
    if str_starts_with_keyword(&state.source[state.offset_utf8..], "of") {
        return None;
    }
    parse_still_lowercase_name_node(state).or_else(|| parse_still_uppercase_name_node(state))
}
fn parse_still_syntax_expression_variable(state: &mut ParseState) -> Option<StillSyntaxExpression> {
    let variable_node = parse_still_syntax_expression_variable_standalone(state)?;
    Some(StillSyntaxExpression::VariableOrCall {
        variable: variable_node,
        arguments: vec![],
    })
}
fn parse_still_syntax_expression_record_or_record_update(
    state: &mut ParseState,
) -> Option<StillSyntaxExpression> {
    if !parse_symbol(state, "{") {
        return None;
    }
    parse_still_whitespace_and_comments(state);
    while parse_symbol(state, ",") {
        parse_still_whitespace_and_comments(state);
    }
    if let Some(spread_key_symbol_range) = parse_symbol_as_range(state, "..") {
        parse_still_whitespace_and_comments(state);
        let maybe_record: Option<StillSyntaxNode<StillSyntaxExpression>> =
            parse_still_syntax_expression_space_separated_node(state);
        parse_still_whitespace_and_comments(state);
        while parse_symbol(state, ",") {
            parse_still_whitespace_and_comments(state);
        }
        let mut fields: Vec<StillSyntaxExpressionField> = Vec::new();
        while let Some(field) = parse_still_syntax_expression_field(state) {
            fields.push(field);
            parse_still_whitespace_and_comments(state);
            while parse_symbol(state, ",") {
                parse_still_whitespace_and_comments(state);
            }
        }
        let _: bool = parse_symbol(state, "}");
        Some(StillSyntaxExpression::RecordUpdate {
            record: maybe_record.map(still_syntax_node_box),
            spread_key_symbol_range,
            fields: fields,
        })
    } else if let Some(field0_name_node) = parse_still_lowercase_name_node(state) {
        parse_still_whitespace_and_comments(state);
        let maybe_field0_value: Option<StillSyntaxNode<StillSyntaxExpression>> =
            parse_still_syntax_expression_space_separated_node(state);
        parse_still_whitespace_and_comments(state);
        while parse_symbol(state, ",") {
            parse_still_whitespace_and_comments(state);
        }
        let mut fields: Vec<StillSyntaxExpressionField> = vec![StillSyntaxExpressionField {
            name: field0_name_node,
            value: maybe_field0_value,
        }];
        while let Some(field) = parse_still_syntax_expression_field(state) {
            fields.push(field);
            parse_still_whitespace_and_comments(state);
            while parse_symbol(state, ",") {
                parse_still_whitespace_and_comments(state);
            }
        }
        let _: bool = parse_symbol(state, "}");
        Some(StillSyntaxExpression::Record(fields))
    } else {
        let _: bool = parse_symbol(state, "}");
        Some(StillSyntaxExpression::Record(vec![]))
    }
}
fn parse_still_syntax_expression_field(
    state: &mut ParseState,
) -> Option<StillSyntaxExpressionField> {
    if state.position.character <= u32::from(state.indent) {
        return None;
    }
    let name_node: StillSyntaxNode<StillName> = parse_still_lowercase_name_node(state)?;
    parse_still_whitespace_and_comments(state);
    let maybe_value: Option<StillSyntaxNode<StillSyntaxExpression>> =
        parse_still_syntax_expression_space_separated_node(state);
    Some(StillSyntaxExpressionField {
        name: name_node,
        value: maybe_value,
    })
}
fn parse_still_syntax_expression_lambda(
    state: &mut ParseState,
) -> Option<StillSyntaxNode<StillSyntaxExpression>> {
    let backslash_key_symbol_range: lsp_types::Range = parse_symbol_as_range(state, "\\")?;
    let mut syntax_before_result_end_position: lsp_types::Position = backslash_key_symbol_range.end;
    parse_still_whitespace_and_comments(state);
    let maybe_parameter: Option<StillSyntaxNode<StillSyntaxPattern>> =
        parse_still_syntax_pattern_node(state);
    if let Some(parameter_node) = &maybe_parameter {
        syntax_before_result_end_position = parameter_node.range.end;
        parse_still_whitespace_and_comments(state);
        // be lenient in allowing , after lambda parameters, even though it's invalid syntax
        while parse_symbol(state, ",") {
            parse_still_whitespace_and_comments(state);
        }
    }
    let maybe_arrow_key_symbol_range: Option<lsp_types::Range> = parse_symbol_as_range(state, "->");
    parse_still_whitespace_and_comments(state);
    let maybe_result: Option<StillSyntaxNode<StillSyntaxExpression>> =
        if state.position.character > u32::from(state.indent) {
            parse_still_syntax_expression_space_separated_node(state)
        } else {
            None
        };
    Some(StillSyntaxNode {
        range: lsp_types::Range {
            start: backslash_key_symbol_range.start,
            end: match &maybe_result {
                None => syntax_before_result_end_position,
                Some(result_node) => result_node.range.end,
            },
        },
        value: StillSyntaxExpression::Lambda {
            parameter: maybe_parameter,
            arrow_key_symbol_range: maybe_arrow_key_symbol_range,
            result: maybe_result.map(still_syntax_node_box),
        },
    })
}
fn parse_still_syntax_expression_case_of(
    state: &mut ParseState,
) -> Option<StillSyntaxNode<StillSyntaxExpression>> {
    let case_keyword_range: lsp_types::Range = parse_still_keyword_as_range(state, "case")?;
    parse_still_whitespace_and_comments(state);
    let maybe_matched: Option<StillSyntaxNode<StillSyntaxExpression>> =
        parse_still_syntax_expression_space_separated_node(state);
    parse_still_whitespace_and_comments(state);
    Some(match parse_symbol_as_range(state, "of") {
        None => StillSyntaxNode {
            range: lsp_types::Range {
                start: case_keyword_range.start,
                end: match &maybe_matched {
                    None => case_keyword_range.end,
                    Some(matched_node) => matched_node.range.end,
                },
            },
            value: StillSyntaxExpression::CaseOf {
                matched: maybe_matched.map(still_syntax_node_box),
                of_keyword_range: None,
                cases: vec![],
            },
        },
        Some(of_keyword_range) => {
            parse_still_whitespace_and_comments(state);
            if state.position.character <= u32::from(state.indent) {
                StillSyntaxNode {
                    range: lsp_types::Range {
                        start: case_keyword_range.start,
                        end: of_keyword_range.end,
                    },
                    value: StillSyntaxExpression::CaseOf {
                        matched: maybe_matched.map(still_syntax_node_box),
                        of_keyword_range: Some(of_keyword_range),
                        cases: vec![],
                    },
                }
            } else {
                parse_state_push_indent(state, state.position.character as u16);
                let mut full_end_position: lsp_types::Position = of_keyword_range.end;
                let mut cases: Vec<StillSyntaxExpressionCase> = Vec::new();
                while let Some(case) = parse_still_syntax_expression_case(state) {
                    full_end_position = case
                        .result
                        .as_ref()
                        .map(|result| result.range.end)
                        .or_else(|| case.arrow_key_symbol_range.as_ref().map(|range| range.end))
                        .unwrap_or(case.pattern.range.end);
                    cases.push(case);
                    parse_still_whitespace_and_comments(state);
                }
                parse_state_pop_indent(state);
                StillSyntaxNode {
                    range: lsp_types::Range {
                        start: case_keyword_range.start,
                        end: full_end_position,
                    },
                    value: StillSyntaxExpression::CaseOf {
                        matched: maybe_matched.map(still_syntax_node_box),
                        of_keyword_range: Some(of_keyword_range),
                        cases,
                    },
                }
            }
        }
    })
}
fn parse_still_syntax_expression_case(state: &mut ParseState) -> Option<StillSyntaxExpressionCase> {
    if state.position.character < u32::from(state.indent) {
        return None;
    }
    let case_pattern_node: StillSyntaxNode<StillSyntaxPattern> =
        parse_still_syntax_pattern_node(state)?;
    parse_still_whitespace_and_comments(state);
    Some(match parse_symbol_as_range(state, "->") {
        None => StillSyntaxExpressionCase {
            pattern: case_pattern_node,
            arrow_key_symbol_range: None,
            result: None,
        },
        Some(arrow_key_symbol_range) => {
            parse_still_whitespace_and_comments(state);
            let maybe_result = parse_still_syntax_expression_space_separated_node(state);
            StillSyntaxExpressionCase {
                pattern: case_pattern_node,
                arrow_key_symbol_range: Some(arrow_key_symbol_range),
                result: maybe_result,
            }
        }
    })
}

fn parse_still_syntax_expression_let_in(
    state: &mut ParseState,
) -> Option<StillSyntaxNode<StillSyntaxExpression>> {
    let let_keyword_range: lsp_types::Range = parse_still_keyword_as_range(state, "let")?;
    parse_still_whitespace_and_comments(state);
    Some(if state.position.character <= u32::from(state.indent) {
        StillSyntaxNode {
            range: let_keyword_range,
            value: StillSyntaxExpression::Let {
                declaration: None,
                result: None,
            },
        }
    } else {
        parse_state_push_indent(state, state.position.character as u16);
        let mut syntax_before_in_key_symbol_end_position: lsp_types::Position =
            let_keyword_range.end;
        let maybe_declaration: Option<StillSyntaxNode<StillSyntaxLetDeclaration>> =
            parse_still_syntax_let_declaration(state);
        if let Some(declaration_node) = &maybe_declaration {
            syntax_before_in_key_symbol_end_position = declaration_node.range.end;
            parse_still_whitespace_and_comments(state);
        }
        parse_state_pop_indent(state);
        parse_still_whitespace_and_comments(state);
        let maybe_result: Option<StillSyntaxNode<StillSyntaxExpression>> =
            parse_still_syntax_expression_space_separated_node(state);
        StillSyntaxNode {
            range: lsp_types::Range {
                start: let_keyword_range.start,
                end: match &maybe_result {
                    None => syntax_before_in_key_symbol_end_position,
                    Some(result_node) => result_node.range.end,
                },
            },
            value: StillSyntaxExpression::Let {
                declaration: maybe_declaration,
                result: maybe_result.map(still_syntax_node_box),
            },
        }
    })
}
fn parse_still_syntax_let_declaration(
    state: &mut ParseState,
) -> Option<StillSyntaxNode<StillSyntaxLetDeclaration>> {
    if state.position.character < u32::from(state.indent) {
        return None;
    }
    parse_still_syntax_let_variable_declaration_node(state)
        .or_else(|| parse_still_syntax_let_destructuring_node(state))
}
fn parse_still_syntax_let_destructuring_node(
    state: &mut ParseState,
) -> Option<StillSyntaxNode<StillSyntaxLetDeclaration>> {
    let pattern_node: StillSyntaxNode<StillSyntaxPattern> = parse_still_syntax_pattern_node(state)?;
    parse_still_whitespace_and_comments(state);
    let maybe_equals_key_symbol_range: Option<lsp_types::Range> = parse_symbol_as_range(state, "=");
    parse_still_whitespace_and_comments(state);
    let maybe_expression: Option<StillSyntaxNode<StillSyntaxExpression>> =
        parse_still_syntax_expression_space_separated_node(state);
    Some(StillSyntaxNode {
        range: lsp_types::Range {
            start: pattern_node.range.start,
            end: match &maybe_expression {
                None => maybe_equals_key_symbol_range
                    .map(|r| r.end)
                    .unwrap_or_else(|| pattern_node.range.end),
                Some(expression_node) => expression_node.range.end,
            },
        },
        value: StillSyntaxLetDeclaration::Destructuring {
            pattern: pattern_node,
            equals_key_symbol_range: maybe_equals_key_symbol_range,
            expression: maybe_expression.map(still_syntax_node_box),
        },
    })
}
fn parse_still_syntax_let_variable_declaration_node(
    state: &mut ParseState,
) -> Option<StillSyntaxNode<StillSyntaxLetDeclaration>> {
    let start_name_node: StillSyntaxNode<StillName> = parse_still_lowercase_name_node(state)?;
    parse_still_whitespace_and_comments(state);
    let maybe_result: Option<StillSyntaxNode<StillSyntaxExpression>> =
        if state.position.character <= u32::from(state.indent) {
            None
        } else {
            parse_still_syntax_expression_space_separated_node(state)
        };
    Some(StillSyntaxNode {
        range: lsp_types::Range {
            start: start_name_node.range.start,
            end: maybe_result
                .as_ref()
                .map(|node| node.range.end)
                .unwrap_or(start_name_node.range.end),
        },
        value: StillSyntaxLetDeclaration::VariableDeclaration {
            start_name: start_name_node,
            result: maybe_result.map(still_syntax_node_box),
        },
    })
}
fn parse_still_syntax_expression_string(state: &mut ParseState) -> Option<StillSyntaxExpression> {
    parse_still_string_triple_quoted(state)
        .map(|content| StillSyntaxExpression::String {
            content: content,
            quoting_style: StillSyntaxStringQuotingStyle::TripleQuoted,
        })
        .or_else(|| {
            parse_still_string_single_quoted(state).map(|content| StillSyntaxExpression::String {
                content: content,
                quoting_style: StillSyntaxStringQuotingStyle::SingleQuoted,
            })
        })
}
fn parse_still_syntax_expression_list(state: &mut ParseState) -> Option<StillSyntaxExpression> {
    if !parse_symbol(state, "[") {
        return None;
    }
    parse_still_whitespace_and_comments(state);
    while parse_symbol(state, ",") {
        parse_still_whitespace_and_comments(state);
    }
    let mut elements: Vec<StillSyntaxNode<StillSyntaxExpression>> = Vec::new();
    while let Some(expression_node) = parse_still_syntax_expression_space_separated_node(state) {
        elements.push(expression_node);
        parse_still_whitespace_and_comments(state);
        while parse_symbol(state, ",") {
            parse_still_whitespace_and_comments(state);
        }
    }
    let _: bool = parse_symbol(state, "]");
    Some(StillSyntaxExpression::Vec(elements))
}
fn parse_still_syntax_expression_parenthesized(
    state: &mut ParseState,
) -> Option<StillSyntaxExpression> {
    if !parse_symbol(state, "(") {
        return None;
    }
    parse_still_whitespace_and_comments(state);
    let maybe_in_parens_0: Option<StillSyntaxNode<StillSyntaxExpression>> =
        parse_still_syntax_expression_space_separated_node(state);
    parse_still_whitespace_and_comments(state);
    let _ = parse_symbol(state, ")");
    Some(StillSyntaxExpression::Parenthesized(
        maybe_in_parens_0.map(still_syntax_node_box),
    ))
}
fn parse_still_syntax_declaration_node(
    state: &mut ParseState,
) -> Option<StillSyntaxNode<StillSyntaxDeclaration>> {
    parse_still_syntax_declaration_choice_type_or_type_alias_node(state).or_else(|| {
        if state.indent != 0 {
            return None;
        }
        parse_still_syntax_declaration_variable_node(state)
    })
}
fn parse_still_syntax_declaration_choice_type_or_type_alias_node(
    state: &mut ParseState,
) -> Option<StillSyntaxNode<StillSyntaxDeclaration>> {
    let type_keyword_range: lsp_types::Range = parse_still_keyword_as_range(state, "type")?;
    parse_still_whitespace_and_comments(state);
    let maybe_alias_keyword_range: Option<lsp_types::Range> = parse_symbol_as_range(state, "alias");
    parse_still_whitespace_and_comments(state);
    let maybe_name_node: Option<StillSyntaxNode<StillName>> =
        parse_still_uppercase_name_node(state);
    parse_still_whitespace_and_comments(state);
    let mut syntax_before_equals_key_symbol_end_location: lsp_types::Position = maybe_name_node
        .as_ref()
        .map(|name_node| name_node.range.end)
        .or_else(|| maybe_alias_keyword_range.map(|range| range.end))
        .unwrap_or(type_keyword_range.end);
    let mut parameters: Vec<StillSyntaxNode<StillName>> = Vec::new();
    while let Some(parameter_node) = parse_still_lowercase_name_node(state) {
        syntax_before_equals_key_symbol_end_location = parameter_node.range.end;
        parameters.push(parameter_node);
        parse_still_whitespace_and_comments(state);
    }
    let maybe_equals_key_symbol_range: Option<lsp_types::Range> = parse_symbol_as_range(state, "=");
    parse_still_whitespace_and_comments(state);
    Some(match maybe_alias_keyword_range {
        Some(alias_keyword_range) => {
            let maybe_type: Option<StillSyntaxNode<StillSyntaxType>> =
                if state.position.character <= u32::from(state.indent) {
                    None
                } else {
                    parse_still_syntax_type_space_separated_node(state)
                };
            let full_end_location: lsp_types::Position = maybe_type
                .as_ref()
                .map(|type_node| type_node.range.end)
                .or_else(|| maybe_equals_key_symbol_range.map(|range| range.end))
                .unwrap_or(syntax_before_equals_key_symbol_end_location);
            StillSyntaxNode {
                range: lsp_types::Range {
                    start: type_keyword_range.start,
                    end: full_end_location,
                },
                value: StillSyntaxDeclaration::TypeAlias {
                    alias_keyword_range: alias_keyword_range,
                    name: maybe_name_node,
                    parameters: parameters,
                    equals_key_symbol_range: maybe_equals_key_symbol_range,
                    type_: maybe_type,
                },
            }
        }
        None => {
            let maybe_variant0_name: Option<StillSyntaxNode<StillName>> =
                parse_still_uppercase_name_node(state);
            parse_still_whitespace_and_comments(state);
            let variant0_maybe_value: Option<StillSyntaxNode<StillSyntaxType>> =
                parse_still_syntax_type_not_space_separated_node(state);
            let mut full_end_position: lsp_types::Position = maybe_variant0_name
                .as_ref()
                .map(|node| node.range.end)
                .or_else(|| maybe_equals_key_symbol_range.map(|range| range.end))
                .unwrap_or(syntax_before_equals_key_symbol_end_location);
            if let Some(value_node) = &variant0_maybe_value {
                full_end_position = value_node.range.end;
                parse_still_whitespace_and_comments(state);
            }
            let mut variant1_up: Vec<StillSyntaxChoiceTypeDeclarationTailingVariant> = Vec::new();
            while let Some(variant_node) =
                parse_still_syntax_choice_type_declaration_trailing_variant_node(state)
            {
                variant1_up.push(variant_node.value);
                full_end_position = variant_node.range.end;
                parse_still_whitespace_and_comments(state);
            }
            StillSyntaxNode {
                range: lsp_types::Range {
                    start: type_keyword_range.start,
                    end: full_end_position,
                },
                value: StillSyntaxDeclaration::ChoiceType {
                    name: maybe_name_node,
                    parameters: parameters,
                    equals_key_symbol_range: maybe_equals_key_symbol_range,
                    variant0_name: maybe_variant0_name,
                    variant0_value: variant0_maybe_value,
                    variant1_up: variant1_up,
                },
            }
        }
    })
}
fn parse_still_syntax_choice_type_declaration_trailing_variant_node(
    state: &mut ParseState,
) -> Option<StillSyntaxNode<StillSyntaxChoiceTypeDeclarationTailingVariant>> {
    let or_key_symbol_range: lsp_types::Range = parse_symbol_as_range(state, "|")?;
    parse_still_whitespace_and_comments(state);
    while parse_symbol(state, "|") {
        parse_still_whitespace_and_comments(state);
    }
    let maybe_name: Option<StillSyntaxNode<StillName>> = parse_still_uppercase_name_node(state);
    parse_still_whitespace_and_comments(state);
    let maybe_value: Option<StillSyntaxNode<StillSyntaxType>> =
        parse_still_syntax_type_not_space_separated_node(state);
    let mut full_end_position: lsp_types::Position = maybe_name
        .as_ref()
        .map(|node| node.range.end)
        .unwrap_or_else(|| or_key_symbol_range.end);
    if let Some(value_node) = &maybe_value {
        full_end_position = value_node.range.end;
        parse_still_whitespace_and_comments(state);
    }
    Some(StillSyntaxNode {
        range: lsp_types::Range {
            start: or_key_symbol_range.start,
            end: full_end_position,
        },
        value: StillSyntaxChoiceTypeDeclarationTailingVariant {
            or_key_symbol_range: or_key_symbol_range,
            name: maybe_name,
            value: maybe_value,
        },
    })
}
fn parse_still_syntax_declaration_variable_node(
    state: &mut ParseState,
) -> Option<StillSyntaxNode<StillSyntaxDeclaration>> {
    let start_name_node: StillSyntaxNode<StillName> = parse_still_lowercase_name_node(state)?;
    parse_still_whitespace_and_comments(state);
    let maybe_result: Option<StillSyntaxNode<StillSyntaxExpression>> =
        if state.position.character <= u32::from(state.indent) {
            None
        } else {
            parse_still_syntax_expression_space_separated_node(state)
        };
    Some(StillSyntaxNode {
        range: lsp_types::Range {
            start: start_name_node.range.start,
            end: maybe_result
                .as_ref()
                .map(|node| node.range.end)
                .unwrap_or(start_name_node.range.end),
        },
        value: StillSyntaxDeclaration::Variable {
            start_name: start_name_node,
            result: maybe_result,
        },
    })
}
fn parse_still_syntax_documented_declaration_followed_by_whitespace_and_comments_and_whatever_indented(
    state: &mut ParseState,
) -> Option<StillSyntaxDocumentedDeclaration> {
    match parse_still_documentation_comment_block_node(state) {
        None => parse_still_syntax_declaration_node(state).map(|declaration_node| {
            parse_still_whitespace_and_comments(state);
            StillSyntaxDocumentedDeclaration {
                documentation: None,
                declaration: Some(declaration_node),
            }
        }),
        Some(documentation_node) => {
            parse_still_whitespace_and_comments(state);
            let maybe_declaration: Option<StillSyntaxNode<StillSyntaxDeclaration>> =
                parse_still_syntax_declaration_node(state);
            parse_still_whitespace_and_comments(state);
            Some(StillSyntaxDocumentedDeclaration {
                documentation: Some(documentation_node),
                declaration: maybe_declaration,
            })
        }
    }
}
fn parse_still_syntax_project(project_source: &str) -> StillSyntaxProject {
    let mut state: ParseState = ParseState {
        source: project_source,
        offset_utf8: 0,
        position: lsp_types::Position {
            line: 0,
            character: 0,
        },
        indent: 0,
        lower_indents_stack: vec![],
        comments: vec![],
    };
    parse_still_whitespace_and_comments(&mut state);
    let mut last_valid_end_offset_utf8: usize = state.offset_utf8;
    let mut last_parsed_was_valid: bool = true;
    let mut declarations: Vec<Result<StillSyntaxDocumentedDeclaration, Box<str>>> =
        Vec::with_capacity(8);
    'parsing_declarations: loop {
        let offset_utf8_before_parsing_documented_declaration: usize = state.offset_utf8;
        match parse_still_syntax_documented_declaration_followed_by_whitespace_and_comments_and_whatever_indented(&mut state) {
            Some(documented_declaration) => {
                if !last_parsed_was_valid {
                    declarations.push(Err(Box::from(&project_source[last_valid_end_offset_utf8..offset_utf8_before_parsing_documented_declaration])));
                }
                last_parsed_was_valid = true;
                declarations.push(Ok(documented_declaration));
                parse_still_whitespace_and_comments(&mut state);
                last_valid_end_offset_utf8 = state.offset_utf8;
            }
            None => {
                last_parsed_was_valid = false;
                parse_before_next_linebreak(&mut state);
                if !parse_linebreak(&mut state) {
                    break 'parsing_declarations;
                }
            }
        }
    }
    if !last_parsed_was_valid {
        declarations.push(Err(Box::from(
            &project_source[last_valid_end_offset_utf8..],
        )));
    }
    StillSyntaxProject {
        comments: state.comments,
        declarations: declarations,
    }
}

fn string_replace_lsp_range(
    string: &mut String,
    range: lsp_types::Range,
    range_length: usize,
    replacement: &str,
) {
    let start_line_offset: usize =
        str_offset_after_n_lsp_linebreaks(string, range.start.line as usize);
    let start_offset: usize = start_line_offset
        + str_starting_utf8_length_for_utf16_length(
            &string[start_line_offset..],
            range.start.character as usize,
        );
    let range_length_utf8: usize =
        str_starting_utf8_length_for_utf16_length(&string[start_offset..], range_length);
    string.replace_range(
        start_offset..(start_offset + range_length_utf8),
        replacement,
    );
}
fn str_offset_after_n_lsp_linebreaks(str: &str, linebreak_count_to_skip: usize) -> usize {
    if linebreak_count_to_skip == 0 {
        return 0;
    }
    let mut offset_after_n_linebreaks: usize = 0;
    let mut encountered_linebreaks: usize = 0;
    'finding_after_n_linebreaks_offset: loop {
        if str[offset_after_n_linebreaks..].starts_with("\r\n") {
            encountered_linebreaks += 1;
            offset_after_n_linebreaks += 2;
            if encountered_linebreaks >= linebreak_count_to_skip {
                break 'finding_after_n_linebreaks_offset;
            }
        } else {
            match str[offset_after_n_linebreaks..].chars().next() {
                None => {
                    break 'finding_after_n_linebreaks_offset;
                }
                // see EOL in https://microsoft.github.io/language-server-protocol/specifications/lsp/3.17/specification/#textDocuments
                Some('\r' | '\n') => {
                    encountered_linebreaks += 1;
                    offset_after_n_linebreaks += 1;
                    if encountered_linebreaks >= linebreak_count_to_skip {
                        break 'finding_after_n_linebreaks_offset;
                    }
                }
                Some(next_char) => {
                    offset_after_n_linebreaks += next_char.len_utf8();
                }
            }
        }
    }
    offset_after_n_linebreaks
}
fn str_starting_utf8_length_for_utf16_length(slice: &str, starting_utf16_length: usize) -> usize {
    let mut utf8_length: usize = 0;
    let mut so_far_length_utf16: usize = 0;
    'traversing_utf16_length: for char in slice.chars() {
        if so_far_length_utf16 >= starting_utf16_length {
            break 'traversing_utf16_length;
        }
        utf8_length += char.len_utf8();
        so_far_length_utf16 += char.len_utf16();
    }
    utf8_length
}
