// lsp still reports this specific error even when it is allowed in the cargo.toml
#![allow(non_upper_case_globals)]
// just to get analysis on still_core, not actually used here
mod still_core;

struct State {
    projects: std::collections::HashMap<std::path::PathBuf, ProjectState>,
    open_still_text_document_uris: std::collections::HashSet<lsp_types::Url>,
}

struct ProjectState {
    source: String,
    syntax: StillSyntaxProject,
    type_aliases: std::collections::HashMap<StillName, TypeAliasInfo>,
    choice_types: std::collections::HashMap<StillName, ChoiceTypeInfo>,
    variable_declarations: std::collections::HashMap<StillName, CompiledVariableDeclarationInfo>,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut full_command = std::env::args().skip(1);
    match full_command.next() {
        None => {
            // consider a help message instead
            lsp_main()
        }
        Some(command) => match command.as_str() {
            "lsp" => lsp_main(),
            "help" | "-h" | "--help" => {
                println!("{command_help}");
                Ok(())
            }
            "build" => {
                let maybe_input_file_path = full_command.next();
                let maybe_output_file_path = full_command.next();
                build_main(
                    maybe_input_file_path.as_ref().map(std::path::Path::new),
                    maybe_output_file_path.as_ref().map(std::path::Path::new),
                );
                Ok(())
            }
            _ => {
                println!("Not a valid command. {command_help}");
                Ok(())
            }
        },
    }
}
const command_help: &str = r"try
  - compile to a rust file: still build [input-file.still [output-file.rs]]
  - start the language server: still lsp
  - print this command help message: still help";

fn build_main(
    maybe_input_file_path: Option<&std::path::Path>,
    maybe_output_file_path: Option<&std::path::Path>,
) {
    let input_file_path: &std::path::Path = match maybe_input_file_path {
        Some(input_file_path) => &input_file_path.with_extension("still"),
        None => std::path::Path::new("still.still"),
    };
    let output_file_path: &std::path::Path = match maybe_output_file_path {
        Some(output_file_path) => &output_file_path.with_extension(".rs"),
        None => &std::path::Path::join(&input_file_path.with_extension(""), "mod.rs"),
    };
    println!("...compiling {input_file_path:?} into {output_file_path:?}.");
    match std::fs::read_to_string(input_file_path) {
        Err(read_error) => {
            eprintln!(
                "was looking for a file with the name {input_file_path:?} but failed: {read_error}"
            );
            std::process::exit(1)
        }
        Ok(project_source) => {
            let still_syntax_project: StillSyntaxProject =
                parse_still_syntax_project(&project_source);
            let mut output_errors: Vec<StillErrorNode> = Vec::new();
            let compiled_project: CompiledProject =
                still_project_compile_to_rust(&mut output_errors, &still_syntax_project);
            for output_error in &output_errors {
                eprintln!(
                    "{input_file_path:?}:{range_start_line}:{range_start_column} {message}",
                    range_start_line = output_error.range.start.line + 1,
                    range_start_column = output_error.range.start.character + 1,
                    message = &output_error.message
                );
            }
            let output_rust_file_string: String = format!(
                "// jump to compiled code by searching for // compiled
{}


// compiled code //


{}",
                include_str!("still_core.rs"),
                prettyplease::unparse(&compiled_project.rust),
            );
            if let Some(output_file_directory_path) = output_file_path.parent()
                && let Err(error) = std::fs::create_dir_all(output_file_directory_path)
            {
                eprintln!(
                    "tried to create the directory containing the output rust file {output_file_path:?} but failed: {}",
                    error
                );
                std::process::exit(1)
            }
            match std::fs::write(output_file_path, output_rust_file_string) {
                Err(write_error) => {
                    eprintln!(
                        "tried to write the output into the rust file {output_file_path:?} but failed: {}",
                        write_error
                    );
                    std::process::exit(1)
                }
                Ok(()) => {
                    if !output_errors.is_empty() {
                        std::process::exit(1)
                    }
                }
            }
        }
    }
}

fn lsp_main() -> Result<(), Box<dyn std::error::Error>> {
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
    let diagnostics_json: serde_json::Value = match serde_json::to_value(diagnostics) {
        Ok(diagnostics_json) => diagnostics_json,
        Err(err) => {
            eprintln!("failed to encode diagnostics {err}");
            return;
        }
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
        let mut updated_source: String = std::mem::take(&mut project_state.source);
        for change in did_change_text_document.content_changes {
            match (change.range, change.range_length) {
                // means full replacement
                (None, None) => {
                    updated_source = change.text;
                }
                (Some(range), Some(range_length)) => {
                    string_replace_lsp_range(
                        &mut updated_source,
                        range,
                        range_length as usize,
                        &change.text,
                    );
                }
                (None, _) | (_, None) => {}
            }
        }
        *project_state = initialize_project_state_from_source(
            connection,
            did_change_text_document.text_document.uri,
            updated_source,
        );
    }
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
            #[allow(clippy::result_large_err)]
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
        uninitialized_project_state()
    }
}

fn initialize_state_for_project_into(
    projects_state: &mut std::collections::HashMap<std::path::PathBuf, ProjectState>,
    project_path: std::path::PathBuf,
) {
    projects_state.insert(project_path, uninitialized_project_state());
}
/// A yet to be initialized dummy [`ProjectState`]
fn uninitialized_project_state() -> ProjectState {
    ProjectState {
        source: String::new(),
        syntax: StillSyntaxProject {
            declarations: vec![],
        },
        type_aliases: std::collections::HashMap::new(),
        choice_types: std::collections::HashMap::new(),
        variable_declarations: std::collections::HashMap::new(),
    }
}
fn initialize_project_state_from_source(
    connection: &lsp_server::Connection,
    url: lsp_types::Url,
    source: String,
) -> ProjectState {
    let mut errors: Vec<StillErrorNode> = Vec::new();
    let parsed_project: StillSyntaxProject = parse_still_syntax_project(&source);
    let compiled_project: CompiledProject =
        still_project_compile_to_rust(&mut errors, &parsed_project);
    publish_diagnostics(
        connection,
        lsp_types::PublishDiagnosticsParams {
            uri: url,
            diagnostics: errors
                .iter()
                .map(still_error_node_to_diagnostic)
                .collect::<Vec<_>>(),
            version: None,
        },
    );
    // TODO output the generated rust
    ProjectState {
        source: source,
        syntax: parsed_project,
        type_aliases: compiled_project.type_aliases,
        choice_types: compiled_project.choice_types,
        variable_declarations: compiled_project.variable_declarations,
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
            &hovered_project_state.type_aliases,
            &hovered_project_state.variable_declarations,
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
                    variants: origin_project_declaration_variants,
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
                            origin_project_declaration_name
                                .as_ref()
                                .map(|n| n.value.as_str()),
                            documentation,
                            origin_project_declaration_parameters,
                            origin_project_declaration_variants,
                        )
                    )
                }
                StillSyntaxDeclaration::TypeAlias {
                    type_keyword_range: _,
                    name: maybe_declaration_name,
                    parameters: origin_project_declaration_parameters,
                    equals_key_symbol_range: _,
                    type_,
                } => present_type_alias_declaration_info_markdown(
                    maybe_declaration_name.as_ref().map(|n| n.value.as_str()),
                    documentation,
                    origin_project_declaration_parameters,
                    type_.as_ref().map(still_syntax_node_as_ref),
                ),
                StillSyntaxDeclaration::Variable {
                    name: _,
                    result: maybe_result_node,
                } => present_variable_declaration_info_markdown(
                    documentation,
                    maybe_result_node
                        .as_ref()
                        .map(|result_node| {
                            still_syntax_expression_type(
                                &hovered_project_state.type_aliases,
                                &hovered_project_state.variable_declarations,
                                still_syntax_node_as_ref(result_node),
                            )
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
            name: _,
            type_: maybe_type_type,
            scope_expression: _,
        } => Some(lsp_types::Hover {
            contents: lsp_types::HoverContents::Markup(lsp_types::MarkupContent {
                kind: lsp_types::MarkupKind::Markdown,
                value: let_declaration_info_markdown(
                    maybe_type_type.as_ref().map(still_syntax_node_as_ref),
                ),
            }),
            range: Some(hovered_symbol_node.range),
        }),
        StillSyntaxSymbol::VariableOrVariant {
            name: hovered_name,
            local_bindings,
        } => {
            if let Some(hovered_local_binding_info) =
                find_local_binding_info(&local_bindings, hovered_name)
            {
                return Some(lsp_types::Hover {
                    contents: lsp_types::HoverContents::Markup(lsp_types::MarkupContent {
                        kind: lsp_types::MarkupKind::Markdown,
                        value: local_binding_info_markdown(
                            hovered_local_binding_info.type_,
                            hovered_local_binding_info.origin,
                        ),
                    }),
                    range: Some(hovered_symbol_node.range),
                });
            }
            let origin_declaration_info_markdown: String =
                if let Some(origin_compiled_variable_declaration_info) = hovered_project_state
                    .variable_declarations
                    .get(hovered_name)
                {
                    present_variable_declaration_info_with_complete_type_markdown(
                        origin_compiled_variable_declaration_info
                            .documentation
                            .as_deref(),
                        origin_compiled_variable_declaration_info.type_.as_ref(),
                    )
                } else {
                    // TODO instead look at type
                    hovered_project_state.choice_types.iter().find_map(
                        |(
                            origin_project_choice_type_declaration_name,
                            origin_project_choice_type_declaration,
                        )| {
                            let any_declared_name_matches_hovered: bool =
                                origin_project_choice_type_declaration.variants.iter().any(
                                    |variant| {
                                        variant.name.as_ref().is_some_and(|name_node| {
                                            name_node.value.as_str() == hovered_name
                                        })
                                    },
                                );
                            if !any_declared_name_matches_hovered {
                                None
                            } else {
                                Some(format!(
                                    "variant in\n{}",
                                    &present_choice_type_declaration_info_markdown(
                                        Some(origin_project_choice_type_declaration_name.as_str()),
                                        origin_project_choice_type_declaration
                                            .documentation
                                            .as_deref(),
                                        &origin_project_choice_type_declaration.parameters,
                                        &origin_project_choice_type_declaration.variants,
                                    )
                                ))
                            }
                        },
                    )?
                };
            Some(lsp_types::Hover {
                contents: lsp_types::HoverContents::Markup(lsp_types::MarkupContent {
                    kind: lsp_types::MarkupKind::Markdown,
                    value: origin_declaration_info_markdown,
                }),
                range: Some(hovered_symbol_node.range),
            })
        }
        StillSyntaxSymbol::Type { name: hovered_name } => {
            let info_markdown: String = if let Some(origin_choice_type_info) =
                hovered_project_state.choice_types.get(hovered_name)
            {
                present_choice_type_declaration_info_markdown(
                    Some(hovered_name),
                    origin_choice_type_info.documentation.as_deref(),
                    &origin_choice_type_info.parameters,
                    &origin_choice_type_info.variants,
                )
            } else if let Some(origin_type_alias_info) =
                hovered_project_state.type_aliases.get(hovered_name)
            {
                present_type_alias_declaration_info_markdown(
                    Some(hovered_name),
                    origin_type_alias_info.documentation.as_deref(),
                    &origin_type_alias_info.parameters,
                    origin_type_alias_info
                        .type_syntax
                        .as_ref()
                        .map(still_syntax_node_as_ref),
                )
            } else {
                return None;
            };
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

fn local_binding_info_markdown(
    maybe_type: Option<StillSyntaxNode<&StillSyntaxType>>,
    origin: LocalBindingOrigin,
) -> String {
    match origin {
        LocalBindingOrigin::PatternVariable(_) => match maybe_type {
            None => "variable introduced in pattern".to_string(),
            Some(type_node) => {
                format!(
                    "variable introduced in pattern
```still
:{}{}:
```
",
                    still_syntax_type_to_string(type_node, 1),
                    match still_syntax_range_line_span(type_node.range) {
                        LineSpan::Single => "",
                        LineSpan::Multiple => "\n    ",
                    }
                )
            }
        },
        LocalBindingOrigin::LetDeclaredVariable { name_range: _ } => {
            let_declaration_info_markdown(maybe_type)
        }
    }
}
fn let_declaration_info_markdown(
    maybe_type_type: Option<StillSyntaxNode<&StillSyntaxType>>,
) -> String {
    match maybe_type_type {
        None => "let variable".to_string(),
        Some(hovered_local_binding_type) => {
            format!(
                "let variable
```still
:{}{}:
```
",
                &still_syntax_type_to_string(hovered_local_binding_type, 1),
                match still_syntax_range_line_span(hovered_local_binding_type.range) {
                    LineSpan::Single => "",
                    LineSpan::Multiple => "\n    ",
                },
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
            &goto_symbol_project_state.type_aliases,
            &goto_symbol_project_state.variable_declarations,
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
                    variants: _,
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
                    type_keyword_range: _,
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
            if let Some(goto_local_binding_info) =
                find_local_binding_info(&local_bindings, goto_name)
            {
                return Some(lsp_types::GotoDefinitionResponse::Scalar(
                    lsp_types::Location {
                        uri: goto_definition_arguments
                            .text_document_position_params
                            .text_document
                            .uri,
                        range: match goto_local_binding_info.origin {
                            LocalBindingOrigin::PatternVariable(range) => range,
                            LocalBindingOrigin::LetDeclaredVariable { name_range } => name_range,
                        },
                    },
                ));
            }
            let declaration_name_range: lsp_types::Range =
                if let Some(origin_variable_declaration_info) = goto_symbol_project_state
                    .variable_declarations
                    .get(goto_name)
                {
                    origin_variable_declaration_info.name_range?
                } else {
                    goto_symbol_project_state.choice_types.values().find_map(
                        |origin_project_choice_type| {
                            origin_project_choice_type
                                .variants
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
                        },
                    )?
                };
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
            let declaration_name_range: lsp_types::Range = if let Some(origin_type_alias_info) =
                goto_symbol_project_state.type_aliases.get(goto_name)
            {
                origin_type_alias_info.name_range?
            } else if let Some(origin_choice_type_info) =
                goto_symbol_project_state.choice_types.get(goto_name)
            {
                origin_choice_type_info.name_range?
            } else {
                return None;
            };
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
            &project_state.type_aliases,
            &project_state.variable_declarations,
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
            type_: _,
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
        } => match find_local_binding_info(&local_bindings, name) {
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
            &to_rename_project_state.type_aliases,
            &to_rename_project_state.variable_declarations,
            rename_arguments.text_document_position.position,
        )?;
    Some(match symbol_to_rename_node.value {
        StillSyntaxSymbol::TypeVariable {
            scope_declaration,
            name: type_variable_to_rename,
        } => {
            let mut all_uses_of_renamed_type_variable: Vec<lsp_types::Range> = Vec::new();
            still_syntax_declaration_uses_of_symbol_into(
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
            state
                .projects
                .iter()
                .filter_map(move |(project_path, project_state)| {
                    let mut all_uses_of_at_docs_project_member: Vec<lsp_types::Range> = Vec::new();
                    still_syntax_project_uses_of_symbol_into(
                        &mut all_uses_of_at_docs_project_member,
                        &project_state.syntax,
                        still_declared_symbol_to_rename,
                    );
                    let still_project_uri: lsp_types::Url =
                        lsp_types::Url::from_file_path(project_path).ok()?;
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
            type_: _,
            scope_expression,
        } => {
            let mut all_uses_of_let_declaration_to_rename: Vec<lsp_types::Range> = Vec::new();
            still_syntax_expression_uses_of_symbol_into(
                &mut all_uses_of_let_declaration_to_rename,
                &[to_rename_name],
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
            if let Some(to_rename_local_binding_info) =
                find_local_binding_info(&local_bindings, to_rename_name)
            {
                let mut all_uses_of_local_binding_to_rename: Vec<lsp_types::Range> = Vec::new();
                match to_rename_local_binding_info.origin {
                    LocalBindingOrigin::PatternVariable(range) => {
                        all_uses_of_local_binding_to_rename.push(range);
                    }
                    LocalBindingOrigin::LetDeclaredVariable { .. } => {
                        // already included in scope expression
                    }
                }
                still_syntax_expression_uses_of_symbol_into(
                    &mut all_uses_of_local_binding_to_rename,
                    &[to_rename_name],
                    to_rename_local_binding_info.scope_expression,
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
                state
                    .projects
                    .iter()
                    .filter_map(|(project_path, project_state)| {
                        let mut all_uses_of_renamed_variable: Vec<lsp_types::Range> = Vec::new();
                        still_syntax_project_uses_of_symbol_into(
                            &mut all_uses_of_renamed_variable,
                            &project_state.syntax,
                            symbol_to_find,
                        );
                        let still_project_uri: lsp_types::Url =
                            lsp_types::Url::from_file_path(project_path).ok()?;
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
            state
                .projects
                .iter()
                .filter_map(|(project_path, project_state)| {
                    let mut all_uses_of_renamed_type: Vec<lsp_types::Range> = Vec::new();
                    still_syntax_project_uses_of_symbol_into(
                        &mut all_uses_of_renamed_type,
                        &project_state.syntax,
                        still_declared_symbol_to_rename,
                    );
                    let still_project_uri: lsp_types::Url =
                        lsp_types::Url::from_file_path(project_path).ok()?;
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
            &to_find_project_state.type_aliases,
            &to_find_project_state.variable_declarations,
            references_arguments.text_document_position.position,
        )?;
    Some(match symbol_to_find_node.value {
        StillSyntaxSymbol::TypeVariable {
            scope_declaration,
            name: type_variable_to_find,
        } => {
            let mut all_uses_of_found_type_variable: Vec<lsp_types::Range> = Vec::new();
            still_syntax_declaration_uses_of_symbol_into(
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
            still_syntax_project_uses_of_symbol_into(
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
            type_: _,
            scope_expression,
        } => {
            let mut all_uses_of_found_let_declaration: Vec<lsp_types::Range> = Vec::new();
            still_syntax_expression_uses_of_symbol_into(
                &mut all_uses_of_found_let_declaration,
                &[to_find_name],
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
            if let Some(to_find_local_binding_info) =
                find_local_binding_info(&local_bindings, to_find_name)
            {
                let mut all_uses_of_found_local_binding: Vec<lsp_types::Range> = Vec::new();
                if references_arguments.context.include_declaration {
                    match to_find_local_binding_info.origin {
                        LocalBindingOrigin::PatternVariable(range) => {
                            all_uses_of_found_local_binding.push(range);
                        }
                        LocalBindingOrigin::LetDeclaredVariable { .. } => {
                            // already included in scope
                        }
                    }
                }
                still_syntax_expression_uses_of_symbol_into(
                    &mut all_uses_of_found_local_binding,
                    &[to_find_name],
                    to_find_local_binding_info.scope_expression,
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
                still_syntax_project_uses_of_symbol_into(
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
            still_syntax_project_uses_of_symbol_into(
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
/// TODO eventually convert to `present_variable_declaration_type_info_markdown`
fn present_variable_declaration_info_markdown(
    maybe_documentation: Option<&str>,
    maybe_variable_type: Option<StillSyntaxNode<&StillSyntaxType>>,
) -> String {
    let description: String = match maybe_variable_type {
        Some(variable_type_node) => {
            format!(
                "project variable
```still
:{}{}:
```
",
                &still_syntax_type_to_string(variable_type_node, 1),
                match still_syntax_range_line_span(variable_type_node.range) {
                    LineSpan::Single => "",
                    LineSpan::Multiple => "\n    ",
                },
            )
        }
        None => "project variable".to_string(),
    };
    match maybe_documentation {
        None => description,
        Some(documentation) => {
            description + "---\n" + documentation_comment_to_markdown(documentation).as_str()
        }
    }
}
fn present_variable_declaration_info_with_complete_type_markdown(
    maybe_documentation: Option<&str>,
    maybe_variable_type: Option<&StillType>,
) -> String {
    // TODO implement actual StillType printing
    present_variable_declaration_info_markdown(
        maybe_documentation,
        maybe_variable_type
            .map(still_type_to_syntax_node)
            .as_ref()
            .map(still_syntax_node_as_ref),
    )
}
fn present_type_alias_declaration_info_markdown(
    maybe_name: Option<&str>,
    maybe_documentation: Option<&str>,
    parameters: &[StillSyntaxNode<StillName>],
    maybe_type: Option<StillSyntaxNode<&StillSyntaxType>>,
) -> String {
    let mut declaration_as_string: String = String::new();
    still_syntax_type_alias_declaration_into(
        &mut declaration_as_string,
        maybe_name,
        parameters,
        maybe_type,
    );
    let description = format!("```still\n{}\n```\n", declaration_as_string);
    match maybe_documentation {
        None => description,
        Some(documentation) => {
            description + "---\n" + documentation_comment_to_markdown(documentation).as_str()
        }
    }
}

fn present_choice_type_declaration_info_markdown(
    maybe_name: Option<&str>, // TODO take &str
    maybe_documentation: Option<&str>,
    parameters: &[StillSyntaxNode<StillName>],
    variants: &[StillSyntaxChoiceTypeVariant],
) -> String {
    let mut declaration_string: String = String::new();
    still_syntax_choice_type_declaration_into(
        &mut declaration_string,
        maybe_name,
        parameters,
        variants,
    );
    let description: String = format!("```still\n{}\n```\n", declaration_string);
    match maybe_documentation {
        None => description,
        Some(documentation) => {
            description + "---\n" + documentation_comment_to_markdown(documentation).as_str()
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
            &completion_project.type_aliases,
            &completion_project.variable_declarations,
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
                                local_binding.type_.as_ref().map(still_syntax_node_as_ref),
                                local_binding.origin,
                            ),
                        },
                    )),
                    ..lsp_types::CompletionItem::default()
                });
            completion_items.extend(local_binding_completions);
            variable_declaration_or_variant_completions_into(
                &completion_project.choice_types,
                &completion_project.variable_declarations,
                &mut completion_items,
            );
            Some(completion_items)
        }
        StillSyntaxSymbol::Type { name: _ } => {
            let mut completion_items: Vec<lsp_types::CompletionItem> = Vec::new();
            type_declaration_completions_into(
                &completion_project.type_aliases,
                &completion_project.choice_types,
                &mut completion_items,
            );
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

fn variable_declaration_or_variant_completions_into(
    choice_types: &std::collections::HashMap<StillName, ChoiceTypeInfo>,
    variable_declarations: &std::collections::HashMap<StillName, CompiledVariableDeclarationInfo>,
    completion_items: &mut Vec<lsp_types::CompletionItem>,
) {
    completion_items.extend(variable_declarations.iter().map(
        |(variable_declaration_name, variable_declaration_info)| lsp_types::CompletionItem {
            label: variable_declaration_name.to_string(),
            kind: Some(lsp_types::CompletionItemKind::FUNCTION),
            documentation: Some(lsp_types::Documentation::MarkupContent(
                lsp_types::MarkupContent {
                    kind: lsp_types::MarkupKind::Markdown,
                    value: present_variable_declaration_info_with_complete_type_markdown(
                        variable_declaration_info.documentation.as_deref(),
                        variable_declaration_info.type_.as_ref(),
                    ),
                },
            )),
            ..lsp_types::CompletionItem::default()
        },
    ));
    completion_items.extend(choice_types.iter().flat_map(
        |(origin_project_choice_type_name, origin_project_choice_type_info)| {
            let info_markdown: String = format!(
                "variant in\n{}",
                present_choice_type_declaration_info_markdown(
                    Some(origin_project_choice_type_name),
                    origin_project_choice_type_info.documentation.as_deref(),
                    &origin_project_choice_type_info.parameters,
                    &origin_project_choice_type_info.variants,
                ),
            );
            origin_project_choice_type_info
                .variants
                .iter()
                .filter_map(|variant| variant.name.as_ref().map(|node| node.value.to_string()))
                .map(move |variant_name: String| lsp_types::CompletionItem {
                    label: variant_name,
                    kind: Some(lsp_types::CompletionItemKind::ENUM_MEMBER),
                    documentation: Some(lsp_types::Documentation::MarkupContent(
                        lsp_types::MarkupContent {
                            kind: lsp_types::MarkupKind::Markdown,
                            value: info_markdown.clone(),
                        },
                    )),
                    ..lsp_types::CompletionItem::default()
                })
        },
    ));
}
fn type_declaration_completions_into(
    type_aliases: &std::collections::HashMap<StillName, TypeAliasInfo>,
    choice_types: &std::collections::HashMap<StillName, ChoiceTypeInfo>,
    completion_items: &mut Vec<lsp_types::CompletionItem>,
) {
    completion_items.extend(choice_types.iter().map(
        |(origin_project_choice_type_name, origin_project_choice_type_info)| {
            lsp_types::CompletionItem {
                label: origin_project_choice_type_name.to_string(),
                kind: Some(lsp_types::CompletionItemKind::ENUM),
                documentation: Some(lsp_types::Documentation::MarkupContent(
                    lsp_types::MarkupContent {
                        kind: lsp_types::MarkupKind::Markdown,
                        value: present_choice_type_declaration_info_markdown(
                            Some(origin_project_choice_type_name),
                            origin_project_choice_type_info.documentation.as_deref(),
                            &origin_project_choice_type_info.parameters,
                            &origin_project_choice_type_info.variants,
                        ),
                    },
                )),
                ..lsp_types::CompletionItem::default()
            }
        },
    ));
    completion_items.extend(
        type_aliases.iter().map(
            |(type_alias_name, type_alias_info)| lsp_types::CompletionItem {
                label: type_alias_name.to_string(),
                kind: Some(lsp_types::CompletionItemKind::STRUCT),
                documentation: Some(lsp_types::Documentation::MarkupContent(
                    lsp_types::MarkupContent {
                        kind: lsp_types::MarkupKind::Markdown,
                        value: present_type_alias_declaration_info_markdown(
                            Some(type_alias_name),
                            type_alias_info.documentation.as_deref(),
                            &type_alias_info.parameters,
                            type_alias_info
                                .type_syntax
                                .as_ref()
                                .map(still_syntax_node_as_ref),
                        ),
                    },
                )),
                ..lsp_types::CompletionItem::default()
            },
        ),
    );
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
                    variants,
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
                            variants
                                .iter()
                                .filter_map(|variant| {
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
                                })
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
                    type_keyword_range: _,
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
                    name: name_node,
                    result: _,
                } => Some(lsp_types::DocumentSymbol {
                    name: name_node.value.to_string(),
                    detail: None,
                    kind: lsp_types::SymbolKind::FUNCTION,
                    tags: None,
                    #[allow(deprecated)]
                    deprecated: None,
                    range: declaration_node.range,
                    selection_range: name_node.range,
                    children: None,
                }),
            })
            .collect::<Vec<_>>(),
    ))
}

fn still_error_node_to_diagnostic(problem: &StillErrorNode) -> lsp_types::Diagnostic {
    lsp_types::Diagnostic {
        range: problem.range,
        severity: Some(lsp_types::DiagnosticSeverity::WARNING),
        code: None,
        code_description: None,
        source: None,
        message: problem.message.to_string(),
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
    WithComment {
        comment: StillSyntaxNode<Box<str>>,
        type_: Option<StillSyntaxNode<Box<StillSyntaxType>>>,
    },
    Function {
        inputs: Vec<StillSyntaxNode<StillSyntaxType>>,
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
/// Fully validated type
#[derive(Clone, Debug)]
enum StillType {
    Variable(StillName),
    Function {
        inputs: Vec<StillType>,
        output: Box<StillType>,
    },
    ChoiceConstruct {
        name: StillName,
        arguments: Vec<StillType>,
    },
    Record(Vec<StillTypeField>),
}
#[derive(Clone, Debug)]
struct StillTypeField {
    name: StillName,
    value: StillType,
}

#[derive(Clone, Debug, PartialEq)]
enum StillSyntaxPattern {
    Char(Option<char>),
    Int(Box<str>),
    String {
        content: String,
        quoting_style: StillSyntaxStringQuotingStyle,
    },
    WithComment {
        comment: StillSyntaxNode<Box<str>>,
        pattern: Option<StillSyntaxNode<Box<StillSyntaxPattern>>>,
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
struct StillSyntaxLetDeclaration {
    name: StillSyntaxNode<StillName>,
    result: Option<StillSyntaxNode<Box<StillSyntaxExpression>>>,
}

#[derive(Clone, Debug, PartialEq)]
enum StillSyntaxExpression {
    VariableOrCall {
        variable: StillSyntaxNode<StillName>,
        arguments: Vec<StillSyntaxNode<StillSyntaxExpression>>,
    },
    Match {
        matched: StillSyntaxNode<Box<StillSyntaxExpression>>,
        // consider splitting into case0, case1_up
        cases: Vec<StillSyntaxExpressionCase>,
    },
    Char(Option<char>),
    Dec(Box<str>),
    Int(Box<str>),
    Lambda {
        parameters: Vec<StillSyntaxNode<StillSyntaxPattern>>,
        arrow_key_symbol_range: Option<lsp_types::Range>,
        result: Option<StillSyntaxNode<Box<StillSyntaxExpression>>>,
    },
    Let {
        declaration: Option<StillSyntaxNode<StillSyntaxLetDeclaration>>,
        result: Option<StillSyntaxNode<Box<StillSyntaxExpression>>>,
    },
    Vec(Vec<StillSyntaxNode<StillSyntaxExpression>>),
    Parenthesized(Option<StillSyntaxNode<Box<StillSyntaxExpression>>>),
    WithComment {
        comment: StillSyntaxNode<Box<str>>,
        expression: Option<StillSyntaxNode<Box<StillSyntaxExpression>>>,
    },
    Typed {
        type_: Option<StillSyntaxNode<StillSyntaxType>>,
        // TODO add second colon range
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
    or_bar_key_symbol_range: lsp_types::Range,
    arrow_key_symbol_range: Option<lsp_types::Range>,
    pattern: Option<StillSyntaxNode<StillSyntaxPattern>>,
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

        variants: Vec<StillSyntaxChoiceTypeVariant>,
    },
    TypeAlias {
        type_keyword_range: lsp_types::Range,
        name: Option<StillSyntaxNode<StillName>>,
        parameters: Vec<StillSyntaxNode<StillName>>,
        equals_key_symbol_range: Option<lsp_types::Range>,
        type_: Option<StillSyntaxNode<StillSyntaxType>>,
    },
    Variable {
        name: StillSyntaxNode<StillName>,
        result: Option<StillSyntaxNode<StillSyntaxExpression>>,
    },
}

#[derive(Clone, Debug, PartialEq)]
struct StillSyntaxChoiceTypeVariant {
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
    declarations: Vec<Result<StillSyntaxDocumentedDeclaration, StillSyntaxNode<Box<str>>>>,
}

#[derive(Clone, Debug, PartialEq)]
struct StillSyntaxDocumentedDeclaration {
    documentation: Option<StillSyntaxNode<Box<str>>>,
    declaration: Option<StillSyntaxNode<StillSyntaxDeclaration>>,
}

struct StillErrorNode {
    range: lsp_types::Range,
    message: Box<str>,
}

fn still_syntax_pattern_type(
    pattern_node: StillSyntaxNode<&StillSyntaxPattern>,
) -> StillSyntaxNode<StillSyntaxType> {
    match pattern_node.value {
        StillSyntaxPattern::Char(_) => still_syntax_node_empty(still_syntax_type_chr),
        StillSyntaxPattern::Int { .. } => still_syntax_node_empty(still_syntax_type_int),
        StillSyntaxPattern::String { .. } => still_syntax_node_empty(still_syntax_type_str),
        StillSyntaxPattern::WithComment {
            comment: _,
            pattern: maybe_pattern_after_comment,
        } => match maybe_pattern_after_comment {
            None => still_syntax_node_empty(StillSyntaxType::Parenthesized(None)),
            Some(pattern_node_after_comment) => {
                still_syntax_pattern_type(still_syntax_node_unbox(pattern_node_after_comment))
            }
        },
        StillSyntaxPattern::Typed {
            type_: maybe_type,
            pattern: _maybe_in_typed,
        } => {
            match maybe_type {
                Some(type_node) => still_syntax_node_as_ref_map(type_node, StillSyntaxType::clone),
                None => {
                    // consider trying regardless for variant
                    still_syntax_node_empty(StillSyntaxType::Parenthesized(None))
                }
            }
        }
        StillSyntaxPattern::Record(fields) => {
            let mut field_types: Vec<StillSyntaxTypeField> = Vec::with_capacity(fields.len());
            for field in fields {
                field_types.push(StillSyntaxTypeField {
                    name: field.name.clone(),
                    value: field.value.as_ref().map(|field_value_node| {
                        still_syntax_pattern_type(still_syntax_node_as_ref(field_value_node))
                    }),
                });
            }
            still_syntax_node_empty(StillSyntaxType::Record(field_types))
        }
    }
}
fn still_syntax_expression_type(
    type_aliases: &std::collections::HashMap<StillName, TypeAliasInfo>,
    variable_declarations: &std::collections::HashMap<StillName, CompiledVariableDeclarationInfo>,
    expression_node: StillSyntaxNode<&StillSyntaxExpression>,
) -> StillSyntaxNode<StillSyntaxType> {
    still_syntax_expression_type_with(
        type_aliases,
        variable_declarations,
        std::rc::Rc::new(std::collections::HashMap::new()),
        expression_node,
    )
}
/// TODO is there a point to these returning partial types?
/// I assume not since this is only used before compiling which needs a full type
fn still_syntax_expression_type_with<'a>(
    type_aliases: &std::collections::HashMap<StillName, TypeAliasInfo>,
    variable_declarations: &std::collections::HashMap<StillName, CompiledVariableDeclarationInfo>,
    local_bindings: std::rc::Rc<
        std::collections::HashMap<&'a str, Option<StillSyntaxNode<StillSyntaxType>>>,
    >,
    expression_node: StillSyntaxNode<&'a StillSyntaxExpression>,
) -> StillSyntaxNode<StillSyntaxType> {
    match expression_node.value {
        StillSyntaxExpression::Typed {
            type_: maybe_type,
            expression: maybe_in_typed,
        } => match maybe_type {
            None => match maybe_in_typed {
                None => StillSyntaxNode {
                    range: expression_node.range,
                    value: StillSyntaxType::Parenthesized(None),
                },
                Some(untyped_node) => match &untyped_node.value {
                    StillSyntaxExpressionUntyped::Variant { .. } => {
                        // consider trying regardless
                        StillSyntaxNode {
                            range: expression_node.range,
                            value: StillSyntaxType::Parenthesized(None),
                        }
                    }
                    StillSyntaxExpressionUntyped::Other(other_expression) => {
                        still_syntax_expression_type_with(
                            type_aliases,
                            variable_declarations,
                            local_bindings,
                            StillSyntaxNode {
                                range: untyped_node.range,
                                value: other_expression,
                            },
                        )
                    }
                },
            },
            Some(type_node) => still_syntax_node_as_ref_map(type_node, StillSyntaxType::clone),
        },
        StillSyntaxExpression::VariableOrCall {
            variable: variable_node,
            arguments,
        } => match local_bindings.get(variable_node.value.as_str()) {
            Some(maybe_variable_type) => {
                let Some(variable_type_node) = maybe_variable_type.as_ref() else {
                    return StillSyntaxNode {
                        range: expression_node.range,
                        value: StillSyntaxType::Parenthesized(None),
                    };
                };
                if arguments.is_empty() {
                    variable_type_node.clone()
                } else {
                    let Some((_inputs, maybe_variable_type_output)) = still_syntax_type_to_function(
                        type_aliases,
                        still_syntax_node_as_ref(variable_type_node),
                    ) else {
                        return variable_type_node.clone();
                    };
                    maybe_variable_type_output.unwrap_or_else(|| StillSyntaxNode {
                        range: expression_node.range,
                        value: StillSyntaxType::Parenthesized(None),
                    })
                }
            }
            None => {
                let Some(maybe_project_variable_info) =
                    variable_declarations.get(variable_node.value.as_str())
                else {
                    return StillSyntaxNode {
                        range: expression_node.range,
                        value: StillSyntaxType::Parenthesized(None),
                    };
                };
                let Some(project_variable_type) = &maybe_project_variable_info.type_ else {
                    return StillSyntaxNode {
                        range: expression_node.range,
                        value: StillSyntaxType::Parenthesized(None),
                    };
                };
                if arguments.is_empty() {
                    still_type_to_syntax_node(project_variable_type)
                } else {
                    let Some((inputs, maybe_variable_type_output)) = still_syntax_type_to_function(
                        type_aliases,
                        still_syntax_node_as_ref(&still_type_to_syntax_node(project_variable_type)),
                    ) else {
                        return still_type_to_syntax_node(project_variable_type);
                    };
                    let Some(variable_type_output) = maybe_variable_type_output else {
                        return StillSyntaxNode {
                            range: expression_node.range,
                            value: StillSyntaxType::Parenthesized(None),
                        };
                    };
                    // optimization possibility: when output contains no type variables,
                    // just return it
                    let mut type_parameter_replacements: std::collections::HashMap<
                        Box<str>,
                        StillSyntaxNode<StillSyntaxType>,
                    > = std::collections::HashMap::new();
                    let argument_types = arguments
                        .iter()
                        .map(|argument_node| {
                            still_syntax_expression_type(
                                type_aliases,
                                variable_declarations,
                                still_syntax_node_as_ref(argument_node),
                            )
                        })
                        .collect::<Vec<_>>();
                    for (parameter_type_node, argument_type_node) in
                        inputs.iter().zip(argument_types.iter())
                    {
                        still_syntax_type_collect_variables_that_are_concrete_into(
                            &mut type_parameter_replacements,
                            type_aliases,
                            still_syntax_node_as_ref(parameter_type_node),
                            still_syntax_node_as_ref(argument_type_node),
                        );
                    }
                    still_syntax_type_replace_variables(
                        // seems inefficient, a function would be better
                        &type_parameter_replacements
                            .iter()
                            .map(|(k, v)| (k.as_ref(), still_syntax_node_as_ref(v)))
                            .collect::<std::collections::HashMap<_, _>>(),
                        still_syntax_node_as_ref(&variable_type_output),
                    )
                }
            }
        },
        StillSyntaxExpression::Match { matched: _, cases } => match cases.iter().find_map(|case| {
            case.result
                .as_ref()
                .map(|result_node| (&case.pattern, result_node))
        }) {
            None => StillSyntaxNode {
                range: expression_node.range,
                value: StillSyntaxType::Parenthesized(None),
            },
            Some((maybe_case_pattern, case_result)) => {
                let mut local_bindings: std::collections::HashMap<
                    &str,
                    Option<StillSyntaxNode<StillSyntaxType>>,
                > = std::rc::Rc::unwrap_or_clone(local_bindings);
                if let Some(case_pattern_node) = maybe_case_pattern {
                    still_syntax_pattern_binding_types_into(
                        &mut local_bindings,
                        still_syntax_node_as_ref(case_pattern_node),
                    );
                }
                still_syntax_expression_type_with(
                    type_aliases,
                    variable_declarations,
                    std::rc::Rc::new(local_bindings),
                    still_syntax_node_as_ref(case_result),
                )
            }
        },
        StillSyntaxExpression::Char(_) => still_syntax_node_empty(still_syntax_type_chr),
        StillSyntaxExpression::Dec(_) => still_syntax_node_empty(still_syntax_type_dec),
        StillSyntaxExpression::Int { .. } => still_syntax_node_empty(still_syntax_type_int),
        StillSyntaxExpression::Lambda {
            parameters,
            arrow_key_symbol_range: _,
            result: maybe_result,
        } => {
            let mut input_types: Vec<StillSyntaxNode<StillSyntaxType>> = Vec::new();
            let mut local_bindings: std::collections::HashMap<
                &str,
                Option<StillSyntaxNode<StillSyntaxType>>,
            > = std::rc::Rc::unwrap_or_clone(local_bindings);
            for parameter_node in parameters {
                input_types.push(still_syntax_pattern_type(still_syntax_node_as_ref(
                    parameter_node,
                )));
                still_syntax_pattern_binding_types_into(
                    &mut local_bindings,
                    still_syntax_node_as_ref(parameter_node),
                );
            }
            still_syntax_node_empty(StillSyntaxType::Function {
                inputs: input_types,
                arrow_key_symbol_range: None,
                output: maybe_result.as_ref().map(|result_node| {
                    still_syntax_node_box(still_syntax_expression_type_with(
                        type_aliases,
                        variable_declarations,
                        std::rc::Rc::new(local_bindings),
                        still_syntax_node_unbox(result_node),
                    ))
                }),
            })
        }
        StillSyntaxExpression::Let {
            declaration: maybe_declaration,
            result: maybe_result,
        } => {
            let Some(result_node) = maybe_result else {
                return StillSyntaxNode {
                    range: expression_node.range,
                    value: StillSyntaxType::Parenthesized(None),
                };
            };
            let local_bindings_with_let: std::rc::Rc<
                std::collections::HashMap<&str, Option<StillSyntaxNode<StillSyntaxType>>>,
            > = match maybe_declaration {
                None => local_bindings,
                Some(declaration_node) => {
                    let local_bindings_without_let: std::rc::Rc<
                        std::collections::HashMap<&str, Option<StillSyntaxNode<StillSyntaxType>>>,
                    > = local_bindings.clone();
                    let mut local_bindings_with_let: std::collections::HashMap<
                        &str,
                        Option<StillSyntaxNode<StillSyntaxType>>,
                    > = (*local_bindings).clone();
                    local_bindings_with_let.insert(
                        &declaration_node.value.name.value,
                        declaration_node
                            .value
                            .result
                            .as_ref()
                            .map(|declaration_result_node| {
                                still_syntax_expression_type_with(
                                    type_aliases,
                                    variable_declarations,
                                    local_bindings_without_let,
                                    still_syntax_node_unbox(declaration_result_node),
                                )
                            }),
                    );
                    std::rc::Rc::new(local_bindings_with_let)
                }
            };
            still_syntax_expression_type_with(
                type_aliases,
                variable_declarations,
                local_bindings_with_let,
                still_syntax_node_unbox(result_node),
            )
        }
        StillSyntaxExpression::Vec(elements) => match elements.as_slice() {
            [] => still_syntax_node_empty(still_syntax_type_vec(StillSyntaxNode {
                range: expression_node.range,
                value: StillSyntaxType::Parenthesized(None),
            })),
            [element0_node, ..] => {
                still_syntax_node_empty(still_syntax_type_vec(still_syntax_expression_type_with(
                    type_aliases,
                    variable_declarations,
                    local_bindings,
                    still_syntax_node_as_ref(element0_node),
                )))
            }
        },
        StillSyntaxExpression::Parenthesized(None) => StillSyntaxNode {
            range: expression_node.range,
            value: StillSyntaxType::Parenthesized(None),
        },
        StillSyntaxExpression::Parenthesized(Some(in_parens)) => still_syntax_expression_type_with(
            type_aliases,
            variable_declarations,
            local_bindings,
            still_syntax_node_unbox(in_parens),
        ),
        StillSyntaxExpression::WithComment {
            comment: _,
            expression: maybe_expression_after_comment,
        } => match maybe_expression_after_comment {
            None => StillSyntaxNode {
                range: expression_node.range,
                value: StillSyntaxType::Parenthesized(None),
            },
            Some(expression_node_after_comment) => still_syntax_expression_type_with(
                type_aliases,
                variable_declarations,
                local_bindings,
                still_syntax_node_unbox(expression_node_after_comment),
            ),
        },
        StillSyntaxExpression::Record(fields) => {
            let mut field_types: Vec<StillSyntaxTypeField> = Vec::new();
            for field in fields {
                field_types.push(StillSyntaxTypeField {
                    name: field.name.clone(),
                    value: field.value.as_ref().map(|field_value_node| {
                        still_syntax_expression_type_with(
                            type_aliases,
                            variable_declarations,
                            local_bindings.clone(),
                            still_syntax_node_as_ref(field_value_node),
                        )
                    }),
                });
            }
            still_syntax_node_empty(StillSyntaxType::Record(field_types))
        }
        StillSyntaxExpression::RecordAccess {
            record: record_node,
            field: maybe_field_name,
        } => {
            let record_type_node: StillSyntaxNode<StillSyntaxType> =
                still_syntax_expression_type_with(
                    type_aliases,
                    variable_declarations,
                    local_bindings,
                    still_syntax_node_unbox(record_node),
                );
            let Some(field_name_node) = maybe_field_name else {
                return record_type_node;
            };
            let Some(record_type_fields) = still_syntax_type_to_record(
                type_aliases,
                still_syntax_node_as_ref(&record_type_node),
            ) else {
                return StillSyntaxNode {
                    range: expression_node.range,
                    value: StillSyntaxType::Parenthesized(None),
                };
            };
            match record_type_fields
                .iter()
                .find(|field| field.name.value == field_name_node.value)
            {
                None => StillSyntaxNode {
                    range: expression_node.range,
                    value: StillSyntaxType::Parenthesized(None),
                },
                Some(accessed_field) => {
                    accessed_field
                        .value
                        .clone()
                        .unwrap_or_else(|| StillSyntaxNode {
                            range: expression_node.range,
                            value: StillSyntaxType::Parenthesized(None),
                        })
                }
            }
        }
        StillSyntaxExpression::RecordUpdate {
            record: maybe_record,
            spread_key_symbol_range: _,
            fields: _,
        } => match maybe_record {
            None => StillSyntaxNode {
                range: expression_node.range,
                value: StillSyntaxType::Parenthesized(None),
            },
            Some(record_node) => still_syntax_expression_type_with(
                type_aliases,
                variable_declarations,
                local_bindings,
                still_syntax_node_unbox(record_node),
            ),
        },
        StillSyntaxExpression::String { .. } => still_syntax_node_empty(still_syntax_type_str),
    }
}
const still_type_chr_name: &str = "chr";
const still_type_chr: StillType = StillType::ChoiceConstruct {
    name: StillName::const_new(still_type_chr_name),
    arguments: vec![],
};
const still_syntax_type_chr: StillSyntaxType = StillSyntaxType::Construct {
    name: still_syntax_node_empty(StillName::const_new(still_type_chr_name)),
    arguments: vec![],
};
const still_type_dec_name: &str = "dec";
const still_type_dec: StillType = StillType::ChoiceConstruct {
    name: StillName::const_new(still_type_dec_name),
    arguments: vec![],
};
const still_syntax_type_dec: StillSyntaxType = StillSyntaxType::Construct {
    name: still_syntax_node_empty(StillName::const_new(still_type_dec_name)),
    arguments: vec![],
};
const still_type_int_name: &str = "int";
const still_type_int: StillType = StillType::ChoiceConstruct {
    name: StillName::const_new(still_type_int_name),
    arguments: vec![],
};
const still_syntax_type_int: StillSyntaxType = StillSyntaxType::Construct {
    name: still_syntax_node_empty(StillName::const_new(still_type_int_name)),
    arguments: vec![],
};
const still_type_str_name: &str = "str";
const still_type_str: StillType = StillType::ChoiceConstruct {
    name: StillName::const_new(still_type_str_name),
    arguments: vec![],
};
const still_syntax_type_str: StillSyntaxType = StillSyntaxType::Construct {
    name: still_syntax_node_empty(StillName::const_new(still_type_str_name)),
    arguments: vec![],
};
const still_type_order_name: &str = "order";
const still_type_order: StillType = StillType::ChoiceConstruct {
    name: StillName::const_new(still_type_order_name),
    arguments: vec![],
};
const still_type_vec_name: &str = "vec";
fn still_type_vec(element_type: StillType) -> StillType {
    StillType::ChoiceConstruct {
        name: StillName::new(still_type_vec_name),
        arguments: vec![element_type],
    }
}
fn still_syntax_type_vec(element_type: StillSyntaxNode<StillSyntaxType>) -> StillSyntaxType {
    StillSyntaxType::Construct {
        name: still_syntax_node_empty(StillName::new(still_type_vec_name)),
        arguments: vec![element_type],
    }
}
const still_type_opt_name: &str = "opt";
fn still_type_opt(value_type: StillType) -> StillType {
    StillType::ChoiceConstruct {
        name: StillName::new(still_type_opt_name),
        arguments: vec![value_type],
    }
}
const still_type_continue_or_exit_name: &str = "continue-or-exit";
fn still_type_continue_or_exit(continue_type: StillType, exit_type: StillType) -> StillType {
    StillType::ChoiceConstruct {
        name: StillName::new(still_type_opt_name),
        arguments: vec![continue_type, exit_type],
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
) -> String {
    let mut builder: String = String::new();
    still_syntax_type_not_parenthesized_into(
        &mut builder,
        indent,
        // pass from parens and slice?
        still_syntax_type,
    );
    builder
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
    type_node: StillSyntaxNode<&StillSyntaxType>,
) {
    match type_node.value {
        StillSyntaxType::Construct {
            name: variable,
            arguments,
        } => {
            let line_span: LineSpan = still_syntax_range_line_span(type_node.range);
            so_far.push_str(&variable.value);
            for argument_node in arguments {
                space_or_linebreak_indented_into(so_far, line_span, next_indent(indent));
                still_syntax_type_parenthesized_if_space_separated_into(
                    so_far,
                    next_indent(indent),
                    still_syntax_type_to_unparenthesized(still_syntax_node_as_ref(argument_node)),
                );
            }
        }
        StillSyntaxType::Function {
            inputs,
            arrow_key_symbol_range: _,
            output: maybe_output,
        } => still_syntax_type_function_into(
            so_far,
            still_syntax_range_line_span(type_node.range),
            indent,
            inputs,
            maybe_output.as_ref().map(still_syntax_node_unbox),
        ),
        StillSyntaxType::Parenthesized(None) => {
            so_far.push_str("()");
        }
        StillSyntaxType::Parenthesized(Some(in_parens)) => {
            still_syntax_type_not_parenthesized_into(
                so_far,
                indent,
                still_syntax_node_unbox(in_parens),
            );
        }
        StillSyntaxType::WithComment {
            comment: comment_node,
            type_: maybe_type_after_comment,
        } => {
            still_syntax_comment_into(so_far, &comment_node.value);
            linebreak_indented_into(so_far, indent);
            if let Some(type_node_after_comment) = maybe_type_after_comment {
                still_syntax_type_not_parenthesized_into(
                    so_far,
                    indent,
                    still_syntax_node_unbox(type_node_after_comment),
                );
            }
        }
        StillSyntaxType::Record(fields) => match fields.split_first() {
            None => {
                so_far.push_str("{}");
            }
            Some((field0, field1_up)) => {
                let line_span: LineSpan = still_syntax_range_line_span(type_node.range);
                so_far.push_str("{ ");
                still_syntax_type_fields_into_string(so_far, indent, line_span, field0, field1_up);
                space_or_linebreak_indented_into(so_far, line_span, indent);
                so_far.push('}');
            }
        },
        StillSyntaxType::Variable(name) => {
            so_far.push_str(name);
        }
    }
}

fn still_syntax_type_function_into(
    so_far: &mut String,
    line_span: LineSpan,
    indent_for_input: usize,
    inputs: &[StillSyntaxNode<StillSyntaxType>],
    maybe_output: Option<StillSyntaxNode<&StillSyntaxType>>,
) {
    so_far.push('\\');
    if line_span == LineSpan::Multiple {
        so_far.push(' ');
    }
    if let Some((input0_node, input1_up)) = inputs.split_first() {
        still_syntax_type_not_parenthesized_into(
            so_far,
            indent_for_input + 2,
            still_syntax_node_as_ref(input0_node),
        );
        for input_node in input1_up {
            if line_span == LineSpan::Multiple {
                linebreak_indented_into(so_far, indent_for_input);
            }
            so_far.push_str(", ");
            still_syntax_type_not_parenthesized_into(
                so_far,
                indent_for_input + 2,
                still_syntax_node_as_ref(input_node),
            );
        }
    }
    space_or_linebreak_indented_into(so_far, line_span, indent_for_input);
    so_far.push_str("> ");
    if let Some(output_node) = maybe_output {
        still_syntax_type_not_parenthesized_into(
            so_far,
            next_indent(indent_for_input + 3),
            output_node,
        );
    }
}

fn still_syntax_type_parenthesized_into(
    so_far: &mut String,
    indent: usize,
    innermost_node: StillSyntaxNode<&StillSyntaxType>,
) {
    so_far.push('(');
    still_syntax_type_not_parenthesized_into(so_far, indent + 1, innermost_node);
    if still_syntax_range_line_span(innermost_node.range) == LineSpan::Multiple {
        linebreak_indented_into(so_far, indent);
    }
    so_far.push(')');
}
fn still_syntax_type_parenthesized_if_space_separated_into(
    so_far: &mut String,
    indent: usize,
    unparenthesized_node: StillSyntaxNode<&StillSyntaxType>,
) {
    let is_space_separated: bool = match unparenthesized_node.value {
        StillSyntaxType::Variable(_)
        | StillSyntaxType::Parenthesized(_)
        | StillSyntaxType::Record(_) => false,
        StillSyntaxType::Function { .. } => true,
        StillSyntaxType::WithComment { .. } => true,
        StillSyntaxType::Construct { name: _, arguments } => !arguments.is_empty(),
    };
    if is_space_separated {
        still_syntax_type_parenthesized_into(so_far, indent, unparenthesized_node);
    } else {
        still_syntax_type_not_parenthesized_into(so_far, indent, unparenthesized_node);
    }
}
/// returns the last syntax end position
fn still_syntax_type_fields_into_string<'a>(
    so_far: &mut String,
    indent: usize,
    line_span: LineSpan,
    field0: &'a StillSyntaxTypeField,
    field1_up: &'a [StillSyntaxTypeField],
) {
    so_far.push_str(&field0.name.value);
    match &field0.value {
        None => {
            so_far.push(' ');
        }
        Some(field0_value_node) => {
            space_or_linebreak_indented_into(
                so_far,
                still_syntax_range_line_span(lsp_types::Range {
                    start: field0.name.range.start,
                    end: field0_value_node.range.end,
                }),
                next_indent(indent + 2),
            );
            still_syntax_type_not_parenthesized_into(
                so_far,
                next_indent(indent + 2),
                still_syntax_node_as_ref(field0_value_node),
            );
        }
    }
    for field in field1_up {
        if line_span == LineSpan::Multiple {
            linebreak_indented_into(so_far, indent);
        }
        so_far.push_str(", ");
        so_far.push_str(&field.name.value);
        match &field.value {
            Some(field_value_node) => {
                space_or_linebreak_indented_into(
                    so_far,
                    still_syntax_range_line_span(lsp_types::Range {
                        start: field.name.range.end,
                        end: field_value_node.range.end,
                    }),
                    next_indent(indent + 2),
                );
                still_syntax_type_not_parenthesized_into(
                    so_far,
                    next_indent(indent + 2),
                    still_syntax_node_as_ref(field_value_node),
                );
            }
            None => {
                so_far.push(' ');
            }
        }
    }
}
fn still_syntax_pattern_into(
    so_far: &mut String,
    indent: usize,
    pattern_node: StillSyntaxNode<&StillSyntaxPattern>,
) {
    match pattern_node.value {
        StillSyntaxPattern::Char(maybe_char) => still_char_into(so_far, *maybe_char),
        StillSyntaxPattern::Int(representation) => {
            still_int_into(so_far, representation);
        }
        StillSyntaxPattern::String {
            content,
            quoting_style,
        } => still_string_into(so_far, *quoting_style, content),
        StillSyntaxPattern::WithComment {
            comment: comment_node,
            pattern: maybe_pattern_after_comment,
        } => {
            still_syntax_comment_into(so_far, &comment_node.value);
            linebreak_indented_into(so_far, indent);
            if let Some(pattern_node_after_comment) = maybe_pattern_after_comment {
                still_syntax_pattern_into(
                    so_far,
                    indent,
                    still_syntax_node_unbox(pattern_node_after_comment),
                );
            }
        }
        StillSyntaxPattern::Typed {
            type_: maybe_type_node,
            pattern: maybe_pattern_node_in_typed,
        } => {
            so_far.push(':');
            if let Some(type_node) = maybe_type_node {
                still_syntax_type_not_parenthesized_into(
                    so_far,
                    1,
                    still_syntax_node_as_ref(type_node),
                );
                if still_syntax_range_line_span(type_node.range) == LineSpan::Multiple {
                    linebreak_indented_into(so_far, indent);
                }
            }
            so_far.push(':');
            if still_syntax_range_line_span(pattern_node.range) == LineSpan::Multiple {
                linebreak_indented_into(so_far, indent);
            }
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
                            space_or_linebreak_indented_into(
                                so_far,
                                still_syntax_range_line_span(pattern_node_in_typed.range),
                                next_indent(indent),
                            );
                            still_syntax_pattern_into(
                                so_far,
                                next_indent(indent),
                                still_syntax_node_unbox(value_node),
                            );
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
                    let line_span = still_syntax_range_line_span(pattern_node.range);
                    so_far.push_str("{ ");
                    so_far.push_str(&field0.name.value);
                    if let Some(field0_value) = &field0.value {
                        space_or_linebreak_indented_into(
                            so_far,
                            still_syntax_range_line_span(lsp_types::Range {
                                start: field0.name.range.start,
                                end: field0_value.range.end,
                            }),
                            next_indent(indent),
                        );
                        still_syntax_pattern_into(
                            so_far,
                            next_indent(indent),
                            still_syntax_node_as_ref(field0_value),
                        );
                    }
                    for field in field_names_iterator {
                        if line_span == LineSpan::Multiple {
                            linebreak_indented_into(so_far, indent);
                        }
                        so_far.push_str(", ");
                        so_far.push_str(&field.name.value);
                        if let Some(field_value) = &field.value {
                            space_or_linebreak_indented_into(
                                so_far,
                                still_syntax_range_line_span(lsp_types::Range {
                                    start: field.name.range.start,
                                    end: field_value.range.end,
                                }),
                                next_indent(indent),
                            );
                            still_syntax_pattern_into(
                                so_far,
                                next_indent(indent),
                                still_syntax_node_as_ref(field_value),
                            );
                        }
                    }
                    space_or_linebreak_indented_into(so_far, line_span, indent);
                    so_far.push('}');
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
fn still_int_into(so_far: &mut String, representation: &str) {
    match representation.parse::<isize>() {
        Err(_) => {
            so_far.push_str(representation);
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
                    && still_syntax_range_line_span(argument0_node.range) == LineSpan::Single
                {
                    LineSpan::Single
                } else {
                    LineSpan::Multiple
                };
                let full_line_span: LineSpan = match line_span_before_argument0 {
                    LineSpan::Multiple => LineSpan::Multiple,
                    LineSpan::Single => still_syntax_range_line_span(expression_node.range),
                };
                space_or_linebreak_indented_into(
                    so_far,
                    line_span_before_argument0,
                    next_indent(indent),
                );
                still_syntax_expression_parenthesized_if_space_separated_into(
                    so_far,
                    next_indent(indent),
                    still_syntax_node_as_ref(argument0_node),
                );
                for argument_node in argument1_up.iter().map(still_syntax_node_as_ref) {
                    space_or_linebreak_indented_into(so_far, full_line_span, next_indent(indent));
                    still_syntax_expression_parenthesized_if_space_separated_into(
                        so_far,
                        next_indent(indent),
                        argument_node,
                    );
                }
            }
        }
        StillSyntaxExpression::Match {
            matched: matched_node,
            cases,
        } => {
            still_syntax_expression_not_parenthesized_into(
                so_far,
                indent,
                still_syntax_node_unbox(matched_node),
            );
            for case in cases {
                linebreak_indented_into(so_far, indent);
                still_syntax_case_into(so_far, indent, case);
            }
        }
        StillSyntaxExpression::Char(maybe_char) => {
            still_char_into(so_far, *maybe_char);
        }
        StillSyntaxExpression::Dec(representation) => match representation.parse::<f64>() {
            Err(_) => {
                so_far.push_str(representation);
            }
            Ok(value) => {
                use std::fmt::Write as _;
                let _ = write!(so_far, "{}", value);
            }
        },
        StillSyntaxExpression::Int(representation) => {
            still_int_into(so_far, representation);
        }
        StillSyntaxExpression::Lambda {
            parameters,
            arrow_key_symbol_range: _,
            result: maybe_result,
        } => {
            so_far.push('\\');
            if let Some((last_parameter_node, parameters_before_last)) = parameters.split_last() {
                let parameters_line_span: LineSpan =
                    still_syntax_range_line_span(lsp_types::Range {
                        start: parameters_before_last
                            .first()
                            .unwrap_or(last_parameter_node)
                            .range
                            .start,
                        end: last_parameter_node.range.end,
                    });
                if parameters_line_span == LineSpan::Multiple {
                    so_far.push(' ');
                }
                for parameter_node in parameters_before_last {
                    still_syntax_pattern_into(
                        so_far,
                        indent + 2,
                        still_syntax_node_as_ref(parameter_node),
                    );
                    if parameters_line_span == LineSpan::Multiple {
                        linebreak_indented_into(so_far, indent);
                    }
                    so_far.push_str(", ");
                }
                still_syntax_pattern_into(
                    so_far,
                    indent + 2,
                    still_syntax_node_as_ref(last_parameter_node),
                );
                space_or_linebreak_indented_into(so_far, parameters_line_span, indent);
            }
            so_far.push('>');
            space_or_linebreak_indented_into(
                so_far,
                still_syntax_range_line_span(expression_node.range),
                next_indent(indent),
            );
            if let Some(result_node) = maybe_result {
                still_syntax_expression_not_parenthesized_into(
                    so_far,
                    next_indent(indent),
                    still_syntax_node_unbox(result_node),
                );
            }
        }
        StillSyntaxExpression::Let {
            declaration: maybe_declaration,
            result: maybe_result,
        } => {
            so_far.push_str("let ");
            if let Some(declaration_node) = maybe_declaration {
                still_syntax_let_declaration_into(
                    so_far,
                    indent,
                    still_syntax_node_as_ref(declaration_node),
                );
            }
            linebreak_indented_into(so_far, indent);
            if let Some(result_node) = maybe_result {
                still_syntax_expression_not_parenthesized_into(
                    so_far,
                    indent,
                    still_syntax_node_unbox(result_node),
                );
            }
        }
        StillSyntaxExpression::Vec(elements) => match elements.split_last() {
            None => {
                so_far.push_str("[]");
            }
            Some((last_element_node, elements_before_last)) => {
                so_far.push_str("[ ");
                let line_span: LineSpan = still_syntax_range_line_span(expression_node.range);
                for element_node in elements_before_last {
                    still_syntax_expression_not_parenthesized_into(
                        so_far,
                        indent + 2,
                        still_syntax_node_as_ref(element_node),
                    );
                    if line_span == LineSpan::Multiple {
                        linebreak_indented_into(so_far, indent);
                    }
                    so_far.push_str(", ");
                }
                still_syntax_expression_not_parenthesized_into(
                    so_far,
                    indent + 2,
                    still_syntax_node_as_ref(last_element_node),
                );
                space_or_linebreak_indented_into(so_far, line_span, indent);
                so_far.push(']');
            }
        },
        StillSyntaxExpression::Parenthesized(None) => {
            so_far.push_str("()");
        }
        StillSyntaxExpression::Parenthesized(Some(in_parens)) => {
            let innermost: StillSyntaxNode<&StillSyntaxExpression> =
                still_syntax_expression_to_unparenthesized(still_syntax_node_unbox(in_parens));
            still_syntax_expression_not_parenthesized_into(so_far, indent, innermost);
        }
        StillSyntaxExpression::WithComment {
            comment: comment_node,
            expression: maybe_expression_after_expression,
        } => {
            still_syntax_comment_into(so_far, &comment_node.value);
            linebreak_indented_into(so_far, indent);
            if let Some(expression_node_after_expression) = maybe_expression_after_expression {
                still_syntax_expression_not_parenthesized_into(
                    so_far,
                    indent,
                    still_syntax_node_unbox(expression_node_after_expression),
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
                    still_syntax_node_as_ref(type_node),
                );
                if still_syntax_range_line_span(type_node.range) == LineSpan::Multiple {
                    linebreak_indented_into(so_far, indent);
                }
            }
            so_far.push(':');
            if let Some(expression_node_in_typed) = maybe_expression {
                if match &expression_node_in_typed.value {
                    StillSyntaxExpressionUntyped::Variant { .. } => false,
                    StillSyntaxExpressionUntyped::Other(_) => {
                        still_syntax_range_line_span(expression_node.range) == LineSpan::Multiple
                    }
                } {
                    linebreak_indented_into(so_far, indent);
                }
                match &expression_node_in_typed.value {
                    StillSyntaxExpressionUntyped::Variant {
                        name: name_node,
                        value: maybe_value,
                    } => {
                        so_far.push_str(&name_node.value);
                        if let Some(value_node) = maybe_value {
                            let line_span: LineSpan =
                                still_syntax_range_line_span(expression_node_in_typed.range);
                            space_or_linebreak_indented_into(
                                so_far,
                                line_span,
                                next_indent(indent),
                            );
                            still_syntax_expression_not_parenthesized_into(
                                so_far,
                                next_indent(indent),
                                still_syntax_node_unbox(value_node),
                            );
                        }
                    }
                    StillSyntaxExpressionUntyped::Other(expression_node_other_in_typed) => {
                        still_syntax_expression_not_parenthesized_into(
                            so_far,
                            indent,
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
                so_far.push_str("{}");
            }
            Some((field0, field1_up)) => {
                let line_span: LineSpan = still_syntax_range_line_span(expression_node.range);
                so_far.push_str("{ ");
                still_syntax_expression_fields_into_string(
                    so_far, indent, line_span, field0, field1_up,
                );
                space_or_linebreak_indented_into(so_far, line_span, indent);
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
            let line_span: LineSpan = still_syntax_range_line_span(expression_node.range);
            so_far.push_str("{ ..");
            if let Some(record_node) = maybe_record {
                still_syntax_expression_not_parenthesized_into(
                    so_far,
                    indent + 4,
                    still_syntax_node_unbox(record_node),
                );
            }
            if let Some((field0, field1_up)) = fields.split_first() {
                space_or_linebreak_indented_into(so_far, line_span, indent);
                so_far.push_str(", ");
                still_syntax_expression_fields_into_string(
                    so_far, indent, line_span, field0, field1_up,
                );
            }
            space_or_linebreak_indented_into(so_far, line_span, indent);
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
fn still_syntax_case_into(so_far: &mut String, indent: usize, case: &StillSyntaxExpressionCase) {
    so_far.push_str("| ");
    if let Some(case_pattern_node) = &case.pattern {
        still_syntax_pattern_into(
            so_far,
            indent + 2,
            still_syntax_node_as_ref(case_pattern_node),
        );
        space_or_linebreak_indented_into(
            so_far,
            still_syntax_range_line_span(case_pattern_node.range),
            indent,
        );
    }
    so_far.push('>');
    match &case.result {
        None => {
            space_or_linebreak_indented_into(
                so_far,
                match &case.pattern {
                    None => LineSpan::Single,
                    Some(case_pattern_node) => {
                        still_syntax_range_line_span(case_pattern_node.range)
                    }
                },
                next_indent(indent),
            );
        }
        Some(result_node) => {
            let result_indent: usize = if result_node.range.start.character
                <= case.or_bar_key_symbol_range.start.character
            {
                indent
            } else {
                next_indent(indent)
            };
            space_or_linebreak_indented_into(
                so_far,
                still_syntax_range_line_span(lsp_types::Range {
                    start: case.or_bar_key_symbol_range.start,
                    end: result_node.range.end,
                }),
                result_indent,
            );
            still_syntax_expression_not_parenthesized_into(
                so_far,
                result_indent,
                still_syntax_node_as_ref(result_node),
            );
        }
    }
}
/// returns the last syntax end position
fn still_syntax_expression_fields_into_string<'a>(
    so_far: &mut String,
    indent: usize,
    line_span: LineSpan,
    field0: &'a StillSyntaxExpressionField,
    field1_up: &'a [StillSyntaxExpressionField],
) {
    so_far.push_str(&field0.name.value);
    if let Some(field0_value_node) = &field0.value {
        space_or_linebreak_indented_into(
            so_far,
            still_syntax_range_line_span(field0_value_node.range),
            next_indent(indent + 2),
        );

        still_syntax_expression_not_parenthesized_into(
            so_far,
            next_indent(indent + 2),
            still_syntax_node_as_ref(field0_value_node),
        );
    }
    for field in field1_up {
        if line_span == LineSpan::Multiple {
            linebreak_indented_into(so_far, indent);
        }
        so_far.push_str(", ");
        so_far.push_str(&field.name.value);
        if let Some(field_value_node) = &field.value {
            space_or_linebreak_indented_into(
                so_far,
                still_syntax_range_line_span(lsp_types::Range {
                    start: field.name.range.end,
                    end: field_value_node.range.end,
                }),
                next_indent(indent + 2),
            );
            still_syntax_expression_not_parenthesized_into(
                so_far,
                next_indent(indent + 2),
                still_syntax_node_as_ref(field_value_node),
            );
        }
    }
}
fn still_syntax_let_declaration_into(
    so_far: &mut String,
    indent: usize,
    let_declaration_node: StillSyntaxNode<&StillSyntaxLetDeclaration>,
) {
    still_syntax_variable_declaration_into(
        so_far,
        indent,
        still_syntax_node_as_ref_map(&let_declaration_node.value.name, StillName::as_str),
        let_declaration_node
            .value
            .result
            .as_ref()
            .map(still_syntax_node_unbox),
    );
}
fn still_syntax_variable_declaration_into(
    so_far: &mut String,
    indent: usize,
    name_node: StillSyntaxNode<&str>,
    maybe_result: Option<StillSyntaxNode<&StillSyntaxExpression>>,
) {
    so_far.push_str(name_node.value);
    match maybe_result {
        None => {
            so_far.push(' ');
        }
        Some(result_node) => {
            let result_node: StillSyntaxNode<&StillSyntaxExpression> =
                still_syntax_expression_to_unparenthesized(result_node);
            let start_on_same_line: bool = match &result_node.value {
                StillSyntaxExpression::Lambda { parameters, .. } => match parameters.first() {
                    Some(first_parameter_node) => {
                        still_syntax_range_line_span(lsp_types::Range {
                            start: first_parameter_node.range.start,
                            end: parameters.last().unwrap_or(first_parameter_node).range.end,
                        }) == LineSpan::Single
                    }
                    None => false,
                },
                StillSyntaxExpression::Typed { .. } => true,
                _ => false,
            };
            if start_on_same_line {
                so_far.push(' ');
                still_syntax_expression_not_parenthesized_into(so_far, indent, result_node);
            } else {
                linebreak_indented_into(so_far, next_indent(indent));
                still_syntax_expression_not_parenthesized_into(
                    so_far,
                    next_indent(indent),
                    result_node,
                );
            }
        }
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
fn still_syntax_range_line_span(range: lsp_types::Range) -> LineSpan {
    if range.start.line == range.end.line {
        LineSpan::Single
    } else {
        LineSpan::Multiple
    }
}

fn still_syntax_expression_parenthesized_into(
    so_far: &mut String,
    indent: usize,
    innermost: StillSyntaxNode<&StillSyntaxExpression>,
) {
    so_far.push('(');
    still_syntax_expression_not_parenthesized_into(so_far, indent + 1, innermost);
    if still_syntax_range_line_span(innermost.range) == LineSpan::Multiple {
        linebreak_indented_into(so_far, indent);
    }
    so_far.push(')');
}
fn still_syntax_expression_parenthesized_if_space_separated_into(
    so_far: &mut String,
    indent: usize,
    expression_node: StillSyntaxNode<&StillSyntaxExpression>,
) {
    let unparenthesized: StillSyntaxNode<&StillSyntaxExpression> =
        still_syntax_expression_to_unparenthesized(expression_node);
    let is_space_separated: bool = match unparenthesized.value {
        StillSyntaxExpression::Lambda { .. } => true,
        StillSyntaxExpression::Let { .. } => true,
        StillSyntaxExpression::VariableOrCall {
            variable: _,
            arguments,
        } => !arguments.is_empty(),
        StillSyntaxExpression::Match { .. } => true,
        StillSyntaxExpression::Typed { .. } => true,
        StillSyntaxExpression::WithComment { .. } => true,
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
        still_syntax_expression_parenthesized_into(so_far, indent, unparenthesized);
    } else {
        still_syntax_expression_not_parenthesized_into(so_far, indent, expression_node);
    }
}

fn still_syntax_project_format(project_state: &ProjectState) -> String {
    let still_syntax_project: &StillSyntaxProject = &project_state.syntax;
    let mut builder: String = String::with_capacity(project_state.source.len());
    // to make it easy to insert above
    builder.push_str("\n\n");
    for documented_declaration_or_err in &still_syntax_project.declarations {
        match documented_declaration_or_err {
            Err(unknown_node) => {
                builder.push_str(&unknown_node.value);
            }
            Ok(documented_declaration) => {
                if let Some(project_documentation_node) = &documented_declaration.documentation {
                    still_syntax_documentation_comment_then_linebreak_into(
                        &mut builder,
                        &project_documentation_node.value,
                    );
                }
                if let Some(declaration_node) = &documented_declaration.declaration {
                    still_syntax_declaration_into(
                        &mut builder,
                        still_syntax_node_as_ref(declaration_node),
                    );
                }
                builder.push_str("\n\n");
            }
        }
    }
    builder
}

fn still_syntax_documentation_comment_then_linebreak_into(so_far: &mut String, content: &str) {
    for line in content.lines() {
        so_far.push('#');
        so_far.push_str(line);
        so_far.push('\n');
    }
    if content.ends_with('\n') {
        so_far.push_str("#\n");
    }
}

fn still_syntax_declaration_into(
    so_far: &mut String,
    declaration_node: StillSyntaxNode<&StillSyntaxDeclaration>,
) {
    match declaration_node.value {
        StillSyntaxDeclaration::ChoiceType {
            name: maybe_name,
            parameters,
            variants,
        } => {
            still_syntax_choice_type_declaration_into(
                so_far,
                maybe_name.as_ref().map(|n| n.value.as_str()),
                parameters,
                variants,
            );
        }
        StillSyntaxDeclaration::TypeAlias {
            type_keyword_range: _,
            name: maybe_name,
            parameters,
            equals_key_symbol_range: _,
            type_: maybe_type,
        } => {
            still_syntax_type_alias_declaration_into(
                so_far,
                maybe_name.as_ref().map(|n| n.value.as_str()),
                parameters,
                maybe_type.as_ref().map(still_syntax_node_as_ref),
            );
        }
        StillSyntaxDeclaration::Variable {
            name: name_node,
            result: maybe_result,
        } => {
            still_syntax_variable_declaration_into(
                so_far,
                0,
                still_syntax_node_as_ref_map(name_node, StillName::as_str),
                maybe_result.as_ref().map(still_syntax_node_as_ref),
            );
        }
    }
}

fn still_syntax_type_alias_declaration_into(
    so_far: &mut String,
    maybe_name: Option<&str>,
    parameters: &[StillSyntaxNode<StillName>],
    maybe_type: Option<StillSyntaxNode<&StillSyntaxType>>,
) {
    so_far.push_str("type ");
    if let Some(name_node) = maybe_name {
        so_far.push_str(name_node);
    }
    for parameter_node in parameters {
        so_far.push(' ');
        so_far.push_str(&parameter_node.value);
    }
    so_far.push_str(" =");
    linebreak_indented_into(so_far, 4);
    if let Some(type_node) = maybe_type {
        still_syntax_type_not_parenthesized_into(so_far, 4, type_node);
    }
}
fn still_syntax_choice_type_declaration_into(
    so_far: &mut String,
    maybe_name: Option<&str>,
    parameters: &[StillSyntaxNode<StillName>],
    variants: &[StillSyntaxChoiceTypeVariant],
) {
    so_far.push_str("choice ");
    if let Some(name) = maybe_name {
        so_far.push_str(name);
    }
    for parameter_node in parameters {
        so_far.push(' ');
        so_far.push_str(&parameter_node.value);
    }
    if variants.is_empty() {
        linebreak_indented_into(so_far, 4);
        so_far.push_str("| ");
    } else {
        for variant in variants {
            linebreak_indented_into(so_far, 4);
            so_far.push_str("| ");
            still_syntax_choice_type_declaration_variant_into(
                so_far,
                variant
                    .name
                    .as_ref()
                    .map(|n| still_syntax_node_as_ref_map(n, StillName::as_str)),
                variant.value.as_ref().map(still_syntax_node_as_ref),
            );
        }
    }
}
fn still_syntax_choice_type_declaration_variant_into(
    so_far: &mut String,
    maybe_variant_name: Option<StillSyntaxNode<&str>>,
    variant_maybe_value: Option<StillSyntaxNode<&StillSyntaxType>>,
) {
    if let Some(variant_name_node) = maybe_variant_name {
        so_far.push_str(variant_name_node.value);
    }
    let Some(variant_last_value_node) = variant_maybe_value else {
        return;
    };
    let line_span: LineSpan = still_syntax_range_line_span(lsp_types::Range {
        start: maybe_variant_name
            .map(|n| n.range.start)
            .unwrap_or(variant_last_value_node.range.start),
        end: variant_last_value_node.range.end,
    });
    if let Some(value_node) = variant_maybe_value {
        space_or_linebreak_indented_into(so_far, line_span, 8);
        still_syntax_type_not_parenthesized_into(so_far, 8, value_node);
    }
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
        type_: Option<StillSyntaxNode<StillSyntaxType>>,
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
#[derive(Clone, Copy)]
struct StillLocalBindingInfo<'a> {
    type_: Option<StillSyntaxNode<&'a StillSyntaxType>>,
    origin: LocalBindingOrigin,
    scope_expression: StillSyntaxNode<&'a StillSyntaxExpression>,
    // TODO add origin range
}
fn find_local_binding_info<'a>(
    local_bindings: &'a StillLocalBindings<'a>,
    to_find: &str,
) -> Option<StillLocalBindingInfo<'a>> {
    local_bindings
        .iter()
        .find_map(|(scope_expression, local_bindings)| {
            local_bindings.iter().find_map(|local_binding| {
                if local_binding.name == to_find {
                    Some(StillLocalBindingInfo {
                        origin: local_binding.origin,
                        type_: local_binding.type_.as_ref().map(still_syntax_node_as_ref),
                        scope_expression: *scope_expression,
                    })
                } else {
                    None
                }
            })
        })
}

fn still_syntax_project_find_symbol_at_position<'a>(
    still_syntax_project: &'a StillSyntaxProject,
    type_aliases: &'a std::collections::HashMap<StillName, TypeAliasInfo>,
    variable_declarations: &std::collections::HashMap<StillName, CompiledVariableDeclarationInfo>,
    position: lsp_types::Position,
) -> Option<StillSyntaxNode<StillSyntaxSymbol<'a>>> {
    still_syntax_project
        .declarations
        .iter()
        .filter_map(|declaration_or_err| declaration_or_err.as_ref().ok())
        .find_map(|documented_declaration| {
            let declaration_node = documented_declaration.declaration.as_ref()?;
            still_syntax_declaration_find_symbol_at_position(
                type_aliases,
                variable_declarations,
                still_syntax_node_as_ref(declaration_node),
                documented_declaration
                    .documentation
                    .as_ref()
                    .map(|node| node.value.as_ref()),
                position,
            )
        })
}

fn still_syntax_declaration_find_symbol_at_position<'a>(
    type_aliases: &std::collections::HashMap<StillName, TypeAliasInfo>,
    variable_declarations: &std::collections::HashMap<StillName, CompiledVariableDeclarationInfo>,
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
                variants,
            } => {
                if let Some(name_node) = maybe_name
                    && lsp_range_includes_position(
                        lsp_types::Range {
                            start: still_syntax_declaration_node.range.start,
                            end: name_node.range.end,
                        },
                        position,
                    )
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
                            variants.iter().find_map(|variant| {
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
                                        still_syntax_type_find_symbol_at_position(
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
                type_keyword_range,
                name: maybe_name,
                parameters,
                equals_key_symbol_range: _,
                type_: maybe_type,
            } => {
                if let Some(name_node) = maybe_name
                    && (lsp_range_includes_position(name_node.range, position)
                        || lsp_range_includes_position(*type_keyword_range, position))
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
                                still_syntax_type_find_symbol_at_position(
                                    still_syntax_declaration_node.value,
                                    still_syntax_node_as_ref(type_node),
                                    position,
                                )
                            })
                        })
                }
            }
            StillSyntaxDeclaration::Variable {
                name: name_node,
                result: maybe_result,
            } => {
                if lsp_range_includes_position(name_node.range, position) {
                    Some(StillSyntaxNode {
                        value: StillSyntaxSymbol::ProjectMemberDeclarationName {
                            name: &name_node.value,
                            declaration: still_syntax_declaration_node,
                            documentation: maybe_documentation,
                        },
                        range: name_node.range,
                    })
                } else {
                    maybe_result.as_ref().and_then(|result_node| {
                        still_syntax_expression_find_symbol_at_position(
                            vec![],
                            type_aliases,
                            variable_declarations,
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

fn still_syntax_pattern_find_symbol_at_position<'a>(
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
                still_syntax_type_find_symbol_at_position(
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
                                still_syntax_pattern_find_symbol_at_position(
                                    scope_declaration,
                                    still_syntax_node_unbox(value),
                                    position,
                                )
                            })
                        }
                    }
                }
            }),
        StillSyntaxPattern::WithComment {
            comment: _,
            pattern: maybe_pattern_after_expression,
        } => maybe_pattern_after_expression
            .as_ref()
            .and_then(|pattern_node_after_expression| {
                still_syntax_pattern_find_symbol_at_position(
                    scope_declaration,
                    still_syntax_node_unbox(pattern_node_after_expression),
                    position,
                )
            }),
        StillSyntaxPattern::Record(fields) => fields.iter().find_map(|field| {
            field.value.as_ref().and_then(|field_value_node| {
                still_syntax_pattern_find_symbol_at_position(
                    scope_declaration,
                    still_syntax_node_as_ref(field_value_node),
                    position,
                )
            })
        }),
        StillSyntaxPattern::String { .. } => None,
    }
}

fn still_syntax_type_find_symbol_at_position<'a>(
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
                        still_syntax_type_find_symbol_at_position(
                            scope_declaration,
                            still_syntax_node_as_ref(argument),
                            position,
                        )
                    })
                }
            }
            StillSyntaxType::Function {
                inputs,
                arrow_key_symbol_range: _,
                output: maybe_output,
            } => inputs
                .iter()
                .find_map(|input_node| {
                    still_syntax_type_find_symbol_at_position(
                        scope_declaration,
                        still_syntax_node_as_ref(input_node),
                        position,
                    )
                })
                .or_else(|| {
                    maybe_output.as_ref().and_then(|output_node| {
                        still_syntax_type_find_symbol_at_position(
                            scope_declaration,
                            still_syntax_node_unbox(output_node),
                            position,
                        )
                    })
                }),
            StillSyntaxType::Parenthesized(None) => None,
            StillSyntaxType::Parenthesized(Some(in_parens)) => {
                still_syntax_type_find_symbol_at_position(
                    scope_declaration,
                    still_syntax_node_unbox(in_parens),
                    position,
                )
            }
            StillSyntaxType::WithComment {
                comment: _,
                type_: maybe_type_after_comment,
            } => maybe_type_after_comment
                .as_ref()
                .and_then(|type_node_after_comment| {
                    still_syntax_type_find_symbol_at_position(
                        scope_declaration,
                        still_syntax_node_unbox(type_node_after_comment),
                        position,
                    )
                }),
            StillSyntaxType::Record(fields) => fields.iter().find_map(|field| {
                field.value.as_ref().and_then(|field_value_node| {
                    still_syntax_type_find_symbol_at_position(
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

#[derive(Clone, Debug, Copy)]
enum LocalBindingOrigin {
    PatternVariable(lsp_types::Range),
    LetDeclaredVariable { name_range: lsp_types::Range },
}
#[derive(Clone, Debug)]
struct StillLocalBinding<'a> {
    name: &'a str,
    type_: Option<StillSyntaxNode<StillSyntaxType>>,
    origin: LocalBindingOrigin,
}

/// TODO swap `type_aliases` parameter to first
fn still_syntax_expression_find_symbol_at_position<'a>(
    mut local_bindings: StillLocalBindings<'a>,
    type_aliases: &std::collections::HashMap<StillName, TypeAliasInfo>,
    variable_declarations: &std::collections::HashMap<StillName, CompiledVariableDeclarationInfo>,
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
                    range: variable_node.range,
                });
            }
            arguments
                .iter()
                .try_fold(local_bindings, |local_bindings, argument| {
                    still_syntax_expression_find_symbol_at_position(
                        local_bindings,
                        type_aliases,
                        variable_declarations,
                        scope_declaration,
                        still_syntax_node_as_ref(argument),
                        position,
                    )
                })
        }
        StillSyntaxExpression::Match {
            matched: matched_node,

            cases,
        } => {
            local_bindings = still_syntax_expression_find_symbol_at_position(
                local_bindings,
                type_aliases,
                variable_declarations,
                scope_declaration,
                still_syntax_node_unbox(matched_node),
                position,
            )?;
            cases
                .iter()
                .try_fold(local_bindings, |mut local_bindings, case| {
                    if let Some(case_pattern_node) = &case.pattern
                        && let Some(found_symbol) = still_syntax_pattern_find_symbol_at_position(
                            scope_declaration,
                            still_syntax_node_as_ref(case_pattern_node),
                            position,
                        )
                    {
                        return std::ops::ControlFlow::Break(found_symbol);
                    }
                    if let Some(case_result_node) = &case.result
                    && // we need to check that the position is actually in that case before committing to mutating local bindings
                    lsp_range_includes_position(case_result_node.range, position)
                    {
                        if let Some(case_pattern_node) = &case.pattern {
                            let mut introduced_bindings: Vec<StillLocalBinding> = Vec::new();
                            still_syntax_pattern_bindings_into(
                                &mut introduced_bindings,
                                still_syntax_node_as_ref(case_pattern_node),
                            );
                            local_bindings.push((
                                still_syntax_node_as_ref(case_result_node),
                                introduced_bindings,
                            ));
                        }
                        still_syntax_expression_find_symbol_at_position(
                            local_bindings,
                            type_aliases,
                            variable_declarations,
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
            parameters,
            arrow_key_symbol_range: _,
            result: maybe_result,
        } => {
            if let Some(found_symbol) = parameters.iter().find_map(|parameter| {
                still_syntax_pattern_find_symbol_at_position(
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
                    for parameter_node in parameters {
                        still_syntax_pattern_bindings_into(
                            &mut introduced_bindings,
                            still_syntax_node_as_ref(parameter_node),
                        );
                    }
                    local_bindings
                        .push((still_syntax_node_unbox(result_node), introduced_bindings));
                    still_syntax_expression_find_symbol_at_position(
                        local_bindings,
                        type_aliases,
                        variable_declarations,
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
                    type_aliases,
                    variable_declarations,
                    &let_declaration_node.value,
                );
            }
            local_bindings.push((still_syntax_expression_node, introduced_bindings));
            local_bindings =
                declarations
                    .iter()
                    .try_fold(local_bindings, |local_bindings, declaration| {
                        still_syntax_let_declaration_find_symbol_at_position(
                            type_aliases,
                            variable_declarations,
                            local_bindings,
                            scope_declaration,
                            still_syntax_expression_node,
                            still_syntax_node_as_ref(declaration),
                            position,
                        )
                    })?;
            match maybe_result {
                Some(result_node) => still_syntax_expression_find_symbol_at_position(
                    local_bindings,
                    type_aliases,
                    variable_declarations,
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
                    still_syntax_expression_find_symbol_at_position(
                        local_bindings,
                        type_aliases,
                        variable_declarations,
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
            still_syntax_expression_find_symbol_at_position(
                local_bindings,
                type_aliases,
                variable_declarations,
                scope_declaration,
                still_syntax_node_unbox(in_parens),
                position,
            )
        }
        StillSyntaxExpression::WithComment {
            comment: _,
            expression: maybe_expression_after_comment,
        } => match maybe_expression_after_comment {
            None => std::ops::ControlFlow::Continue(local_bindings),
            Some(expression_node_after_comment) => still_syntax_expression_find_symbol_at_position(
                local_bindings,
                type_aliases,
                variable_declarations,
                scope_declaration,
                still_syntax_node_unbox(expression_node_after_comment),
                position,
            ),
        },
        StillSyntaxExpression::Typed {
            type_: maybe_type,
            expression: maybe_expression_in_typed,
        } => {
            if let Some(found) = maybe_type.as_ref().and_then(|type_node| {
                still_syntax_type_find_symbol_at_position(
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
                            Some(value_node) => still_syntax_expression_find_symbol_at_position(
                                local_bindings,
                                type_aliases,
                                variable_declarations,
                                scope_declaration,
                                still_syntax_node_unbox(value_node),
                                position,
                            ),
                            None => std::ops::ControlFlow::Continue(local_bindings),
                        }
                    }
                    StillSyntaxExpressionUntyped::Other(other_expression_in_typed) => {
                        still_syntax_expression_find_symbol_at_position(
                            local_bindings,
                            type_aliases,
                            variable_declarations,
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
                    Some(field_value_node) => still_syntax_expression_find_symbol_at_position(
                        local_bindings,
                        type_aliases,
                        variable_declarations,
                        scope_declaration,
                        still_syntax_node_as_ref(field_value_node),
                        position,
                    ),
                    None => std::ops::ControlFlow::Continue(local_bindings),
                })
        }
        StillSyntaxExpression::RecordAccess { record, field: _ } => {
            still_syntax_expression_find_symbol_at_position(
                local_bindings,
                type_aliases,
                variable_declarations,
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
                return still_syntax_expression_find_symbol_at_position(
                    local_bindings,
                    type_aliases,
                    variable_declarations,
                    scope_declaration,
                    still_syntax_node_unbox(record_node),
                    position,
                );
            }
            fields
                .iter()
                .try_fold(local_bindings, |local_bindings, field| match &field.value {
                    Some(field_value_node) => still_syntax_expression_find_symbol_at_position(
                        local_bindings,
                        type_aliases,
                        variable_declarations,
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

fn still_syntax_let_declaration_find_symbol_at_position<'a>(
    type_aliases: &std::collections::HashMap<StillName, TypeAliasInfo>,
    variable_declarations: &std::collections::HashMap<StillName, CompiledVariableDeclarationInfo>,
    local_bindings: StillLocalBindings<'a>,
    scope_declaration: &'a StillSyntaxDeclaration,
    scope_expression: StillSyntaxNode<&'a StillSyntaxExpression>,
    still_syntax_let_declaration_node: StillSyntaxNode<&'a StillSyntaxLetDeclaration>,
    position: lsp_types::Position,
) -> std::ops::ControlFlow<StillSyntaxNode<StillSyntaxSymbol<'a>>, StillLocalBindings<'a>> {
    if !lsp_range_includes_position(still_syntax_let_declaration_node.range, position) {
        return std::ops::ControlFlow::Continue(local_bindings);
    }
    if lsp_range_includes_position(still_syntax_let_declaration_node.value.name.range, position) {
        return std::ops::ControlFlow::Break(StillSyntaxNode {
            value: StillSyntaxSymbol::LetDeclarationName {
                name: &still_syntax_let_declaration_node.value.name.value,
                type_: still_syntax_let_declaration_node
                    .value
                    .result
                    .as_ref()
                    .map(|result_node| {
                        still_syntax_expression_type(
                            type_aliases,
                            variable_declarations,
                            still_syntax_node_unbox(result_node),
                        )
                    }),
                scope_expression: scope_expression,
            },
            range: still_syntax_let_declaration_node.value.name.range,
        });
    }
    match &still_syntax_let_declaration_node.value.result {
        Some(result_node) => still_syntax_expression_find_symbol_at_position(
            local_bindings,
            type_aliases,
            variable_declarations,
            scope_declaration,
            still_syntax_node_unbox(result_node),
            position,
        ),
        None => std::ops::ControlFlow::Continue(local_bindings),
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

fn still_syntax_project_uses_of_symbol_into(
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
            still_syntax_declaration_uses_of_symbol_into(
                uses_so_far,
                &declaration_node.value,
                symbol_to_collect_uses_of,
            );
        }
    }
}

fn still_syntax_declaration_uses_of_symbol_into(
    uses_so_far: &mut Vec<lsp_types::Range>,
    still_syntax_declaration: &StillSyntaxDeclaration,
    symbol_to_collect_uses_of: StillSymbolToReference,
) {
    match still_syntax_declaration {
        StillSyntaxDeclaration::ChoiceType {
            name: maybe_name,
            parameters,
            variants,
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
            for variant in variants {
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
                    still_syntax_type_uses_of_symbol_into(
                        uses_so_far,
                        still_syntax_node_as_ref(variant0_value),
                        symbol_to_collect_uses_of,
                    );
                }
            }
        }
        StillSyntaxDeclaration::TypeAlias {
            type_keyword_range: _,
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
                still_syntax_type_uses_of_symbol_into(
                    uses_so_far,
                    still_syntax_node_as_ref(type_node),
                    symbol_to_collect_uses_of,
                );
            }
        }
        StillSyntaxDeclaration::Variable {
            name: name_node,
            result: maybe_result,
        } => {
            if symbol_to_collect_uses_of
                == (StillSymbolToReference::VariableOrVariant {
                    name: &name_node.value,

                    including_declaration_name: true,
                })
            {
                uses_so_far.push(name_node.range);
            }
            if let Some(result_node) = maybe_result {
                still_syntax_expression_uses_of_symbol_into(
                    uses_so_far,
                    &[],
                    still_syntax_node_as_ref(result_node),
                    symbol_to_collect_uses_of,
                );
            }
        }
    }
}

fn still_syntax_type_uses_of_symbol_into(
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
                still_syntax_type_uses_of_symbol_into(
                    uses_so_far,
                    still_syntax_node_as_ref(argument),
                    symbol_to_collect_uses_of,
                );
            }
        }
        StillSyntaxType::Function {
            inputs,
            arrow_key_symbol_range: _,
            output: maybe_output,
        } => {
            for input in inputs {
                still_syntax_type_uses_of_symbol_into(
                    uses_so_far,
                    still_syntax_node_as_ref(input),
                    symbol_to_collect_uses_of,
                );
            }
            if let Some(output_node) = maybe_output {
                still_syntax_type_uses_of_symbol_into(
                    uses_so_far,
                    still_syntax_node_unbox(output_node),
                    symbol_to_collect_uses_of,
                );
            }
        }
        StillSyntaxType::Parenthesized(None) => {}
        StillSyntaxType::Parenthesized(Some(in_parens)) => {
            still_syntax_type_uses_of_symbol_into(
                uses_so_far,
                still_syntax_node_unbox(in_parens),
                symbol_to_collect_uses_of,
            );
        }
        StillSyntaxType::WithComment {
            comment: _,
            type_: maybe_type_after_comment,
        } => {
            if let Some(type_node_after_comment) = maybe_type_after_comment {
                still_syntax_type_uses_of_symbol_into(
                    uses_so_far,
                    still_syntax_node_unbox(type_node_after_comment),
                    symbol_to_collect_uses_of,
                );
            }
        }
        StillSyntaxType::Record(fields) => {
            for field in fields {
                if let Some(field_value_node) = &field.value {
                    still_syntax_type_uses_of_symbol_into(
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

fn still_syntax_expression_uses_of_symbol_into(
    uses_so_far: &mut Vec<lsp_types::Range>,
    local_bindings: &[&str],
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
                && local_bindings.contains(&name)
            {
                uses_so_far.push(still_syntax_expression_node.range);
            }
            for argument_node in arguments {
                still_syntax_expression_uses_of_symbol_into(
                    uses_so_far,
                    local_bindings,
                    still_syntax_node_as_ref(argument_node),
                    symbol_to_collect_uses_of,
                );
            }
        }
        StillSyntaxExpression::Match {
            matched: matched_node,
            cases,
        } => {
            still_syntax_expression_uses_of_symbol_into(
                uses_so_far,
                local_bindings,
                still_syntax_node_unbox(matched_node),
                symbol_to_collect_uses_of,
            );
            for case in cases {
                if let Some(case_pattern_node) = &case.pattern {
                    still_syntax_pattern_uses_of_symbol_into(
                        uses_so_far,
                        still_syntax_node_as_ref(case_pattern_node),
                        symbol_to_collect_uses_of,
                    );
                }
                if let Some(case_result_node) = &case.result {
                    let mut local_bindings_including_from_case_pattern: Vec<&str> =
                        local_bindings.to_vec();
                    if let Some(case_pattern_node) = &case.pattern {
                        still_syntax_pattern_binding_names_into(
                            &mut local_bindings_including_from_case_pattern,
                            still_syntax_node_as_ref(case_pattern_node),
                        );
                    }
                    still_syntax_expression_uses_of_symbol_into(
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
            parameters,
            arrow_key_symbol_range: _,
            result: maybe_result,
        } => {
            for parameter_node in parameters {
                still_syntax_pattern_uses_of_symbol_into(
                    uses_so_far,
                    still_syntax_node_as_ref(parameter_node),
                    symbol_to_collect_uses_of,
                );
            }
            if let Some(result_node) = maybe_result {
                let mut local_bindings_including_from_lambda_parameters: Vec<&str> =
                    local_bindings.to_vec();
                for parameter_node in parameters {
                    still_syntax_pattern_binding_names_into(
                        &mut local_bindings_including_from_lambda_parameters,
                        still_syntax_node_as_ref(parameter_node),
                    );
                }
                still_syntax_expression_uses_of_symbol_into(
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
            let mut local_bindings_including_let_declaration_introduced: Vec<&str> =
                local_bindings.to_vec();
            if let Some(let_declaration_node) = maybe_declaration {
                local_bindings_including_let_declaration_introduced
                    .push(&let_declaration_node.value.name.value);
            }
            if let Some(let_declaration_node) = maybe_declaration {
                still_syntax_let_declaration_uses_of_symbol_into(
                    uses_so_far,
                    &local_bindings_including_let_declaration_introduced,
                    &let_declaration_node.value,
                    symbol_to_collect_uses_of,
                );
            }
            if let Some(result) = maybe_result {
                still_syntax_expression_uses_of_symbol_into(
                    uses_so_far,
                    &local_bindings_including_let_declaration_introduced,
                    still_syntax_node_unbox(result),
                    symbol_to_collect_uses_of,
                );
            }
        }
        StillSyntaxExpression::Vec(elements) => {
            for element_node in elements {
                still_syntax_expression_uses_of_symbol_into(
                    uses_so_far,
                    local_bindings,
                    still_syntax_node_as_ref(element_node),
                    symbol_to_collect_uses_of,
                );
            }
        }
        StillSyntaxExpression::Parenthesized(None) => {}
        StillSyntaxExpression::Parenthesized(Some(in_parens)) => {
            still_syntax_expression_uses_of_symbol_into(
                uses_so_far,
                local_bindings,
                still_syntax_node_unbox(in_parens),
                symbol_to_collect_uses_of,
            );
        }
        StillSyntaxExpression::WithComment {
            comment: _,
            expression: maybe_expression_after_comment,
        } => {
            if let Some(expression_node_after_comment) = maybe_expression_after_comment {
                still_syntax_expression_uses_of_symbol_into(
                    uses_so_far,
                    local_bindings,
                    still_syntax_node_unbox(expression_node_after_comment),
                    symbol_to_collect_uses_of,
                );
            }
        }
        StillSyntaxExpression::Typed {
            type_: maybe_type,
            expression: maybe_expression_in_typed,
        } => {
            if let Some(type_node) = maybe_type {
                still_syntax_type_uses_of_symbol_into(
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
                            still_syntax_expression_uses_of_symbol_into(
                                uses_so_far,
                                local_bindings,
                                still_syntax_node_unbox(value_node),
                                symbol_to_collect_uses_of,
                            );
                        }
                    }
                    StillSyntaxExpressionUntyped::Other(other_expression_in_typed) => {
                        still_syntax_expression_uses_of_symbol_into(
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
                    still_syntax_expression_uses_of_symbol_into(
                        uses_so_far,
                        local_bindings,
                        still_syntax_node_as_ref(field_value_node),
                        symbol_to_collect_uses_of,
                    );
                }
            }
        }
        StillSyntaxExpression::RecordAccess { record, field: _ } => {
            still_syntax_expression_uses_of_symbol_into(
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
                still_syntax_expression_uses_of_symbol_into(
                    uses_so_far,
                    local_bindings,
                    still_syntax_node_unbox(record_node),
                    symbol_to_collect_uses_of,
                );
            }
            for field in fields {
                if let Some(field_value_node) = &field.value {
                    still_syntax_expression_uses_of_symbol_into(
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

fn still_syntax_let_declaration_uses_of_symbol_into(
    uses_so_far: &mut Vec<lsp_types::Range>,
    local_bindings: &[&str],
    still_syntax_let_declaration: &StillSyntaxLetDeclaration,
    symbol_to_collect_uses_of: StillSymbolToReference,
) {
    if symbol_to_collect_uses_of
        == (StillSymbolToReference::LocalBinding {
            name: &still_syntax_let_declaration.name.value,
            including_let_declaration_name: true,
        })
    {
        uses_so_far.push(still_syntax_let_declaration.name.range);
        return;
    }
    if let Some(result_node) = &still_syntax_let_declaration.result {
        still_syntax_expression_uses_of_symbol_into(
            uses_so_far,
            local_bindings,
            still_syntax_node_unbox(result_node),
            symbol_to_collect_uses_of,
        );
    }
}

fn still_syntax_pattern_uses_of_symbol_into(
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
                still_syntax_type_uses_of_symbol_into(
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
                            still_syntax_pattern_uses_of_symbol_into(
                                uses_so_far,
                                still_syntax_node_unbox(value),
                                symbol_to_collect_uses_of,
                            );
                        }
                    }
                }
            }
        }
        StillSyntaxPattern::WithComment {
            comment: _,
            pattern: maybe_pattern_after_comment,
        } => {
            if let Some(pattern_node_after_comment) = maybe_pattern_after_comment {
                still_syntax_pattern_uses_of_symbol_into(
                    uses_so_far,
                    still_syntax_node_unbox(pattern_node_after_comment),
                    symbol_to_collect_uses_of,
                );
            }
        }
        StillSyntaxPattern::Record(fields) => {
            for value in fields.iter().filter_map(|field| field.value.as_ref()) {
                still_syntax_pattern_uses_of_symbol_into(
                    uses_so_far,
                    still_syntax_node_as_ref(value),
                    symbol_to_collect_uses_of,
                );
            }
        }
        StillSyntaxPattern::String { .. } => {}
    }
}

fn still_syntax_let_declaration_introduced_bindings_into<'a>(
    bindings_so_far: &mut Vec<StillLocalBinding<'a>>,
    type_aliases: &std::collections::HashMap<StillName, TypeAliasInfo>,
    variable_declarations: &std::collections::HashMap<StillName, CompiledVariableDeclarationInfo>,
    still_syntax_let_declaration: &'a StillSyntaxLetDeclaration,
) {
    bindings_so_far.push(StillLocalBinding {
        name: &still_syntax_let_declaration.name.value,
        origin: LocalBindingOrigin::LetDeclaredVariable {
            name_range: still_syntax_let_declaration.name.range,
        },
        type_: still_syntax_let_declaration
            .result
            .as_ref()
            .map(|result_node| {
                still_syntax_expression_type_with(
                    type_aliases,
                    variable_declarations,
                    // this is inefficient to do for every let variable
                    std::rc::Rc::new(
                        bindings_so_far
                            .iter()
                            .map(|binding| (binding.name, binding.type_.clone()))
                            .collect::<std::collections::HashMap<_, _>>(),
                    ),
                    still_syntax_node_unbox(result_node),
                )
            }),
    });
}

fn still_syntax_pattern_bindings_into<'a>(
    bindings_so_far: &mut Vec<StillLocalBinding<'a>>,
    still_syntax_pattern_node: StillSyntaxNode<&'a StillSyntaxPattern>,
) {
    match still_syntax_pattern_node.value {
        StillSyntaxPattern::Char(_) => {}
        StillSyntaxPattern::Int { .. } => {}
        StillSyntaxPattern::String { .. } => {}
        StillSyntaxPattern::Typed {
            type_: maybe_type,
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
                            type_: maybe_type.clone(),
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
        StillSyntaxPattern::WithComment {
            comment: _,
            pattern: maybe_pattern_after_comment,
        } => {
            if let Some(pattern_node_after_comment) = maybe_pattern_after_comment {
                still_syntax_pattern_bindings_into(
                    bindings_so_far,
                    still_syntax_node_unbox(pattern_node_after_comment),
                );
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
    }
}
fn still_syntax_pattern_binding_names_into<'a>(
    bindings_so_far: &mut Vec<&'a str>,
    still_syntax_pattern_node: StillSyntaxNode<&'a StillSyntaxPattern>,
) {
    match still_syntax_pattern_node.value {
        StillSyntaxPattern::Char(_) => {}
        StillSyntaxPattern::Int { .. } => {}
        StillSyntaxPattern::String { .. } => {}
        StillSyntaxPattern::Typed {
            type_: _,
            pattern: maybe_pattern_node_in_typed,
        } => {
            if let Some(pattern_node_in_typed) = maybe_pattern_node_in_typed {
                match &pattern_node_in_typed.value {
                    StillSyntaxPatternUntyped::Ignored => {}
                    StillSyntaxPatternUntyped::Variable(variable) => {
                        bindings_so_far.push(variable);
                    }
                    StillSyntaxPatternUntyped::Variant {
                        name: _,
                        value: maybe_value,
                    } => {
                        if let Some(value_node) = maybe_value {
                            still_syntax_pattern_binding_names_into(
                                bindings_so_far,
                                still_syntax_node_unbox(value_node),
                            );
                        }
                    }
                }
            }
        }
        StillSyntaxPattern::WithComment {
            comment: _,
            pattern: maybe_pattern_after_comment,
        } => {
            if let Some(pattern_node_after_comment) = maybe_pattern_after_comment {
                still_syntax_pattern_binding_names_into(
                    bindings_so_far,
                    still_syntax_node_unbox(pattern_node_after_comment),
                );
            }
        }
        StillSyntaxPattern::Record(fields) => {
            for field in fields {
                if let Some(field_value_node) = &field.value {
                    still_syntax_pattern_binding_names_into(
                        bindings_so_far,
                        still_syntax_node_as_ref(field_value_node),
                    );
                }
            }
        }
    }
}
fn still_syntax_pattern_binding_types_into<'a>(
    bindings_so_far: &mut std::collections::HashMap<
        &'a str,
        Option<StillSyntaxNode<StillSyntaxType>>,
    >,
    still_syntax_pattern_node: StillSyntaxNode<&'a StillSyntaxPattern>,
) {
    match still_syntax_pattern_node.value {
        StillSyntaxPattern::Char(_) => {}
        StillSyntaxPattern::Int { .. } => {}
        StillSyntaxPattern::String { .. } => {}
        StillSyntaxPattern::Typed {
            type_: maybe_type,
            pattern: maybe_pattern_node_in_typed,
        } => {
            if let Some(pattern_node_in_typed) = maybe_pattern_node_in_typed {
                match &pattern_node_in_typed.value {
                    StillSyntaxPatternUntyped::Ignored => {}
                    StillSyntaxPatternUntyped::Variable(variable) => {
                        bindings_so_far.insert(variable, maybe_type.clone());
                    }
                    StillSyntaxPatternUntyped::Variant {
                        name: _,
                        value: maybe_value,
                    } => {
                        if let Some(value_node) = maybe_value {
                            still_syntax_pattern_binding_types_into(
                                bindings_so_far,
                                still_syntax_node_unbox(value_node),
                            );
                        }
                    }
                }
            }
        }
        StillSyntaxPattern::WithComment {
            comment: _,
            pattern: maybe_pattern_after_comment,
        } => {
            if let Some(pattern_node_after_comment) = maybe_pattern_after_comment {
                still_syntax_pattern_binding_types_into(
                    bindings_so_far,
                    still_syntax_node_unbox(pattern_node_after_comment),
                );
            }
        }
        StillSyntaxPattern::Record(fields) => {
            for field in fields {
                if let Some(field_value_node) = &field.value {
                    still_syntax_pattern_binding_types_into(
                        bindings_so_far,
                        still_syntax_node_as_ref(field_value_node),
                    );
                }
            }
        }
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
            name: name_node,
            result: maybe_result,
        } => {
            highlighted_so_far.push(StillSyntaxNode {
                range: name_node.range,
                value: StillSyntaxHighlightKind::DeclaredVariable,
            });
            if let Some(result_node) = maybe_result {
                still_syntax_highlight_expression_into(
                    highlighted_so_far,
                    still_syntax_node_as_ref(result_node),
                );
            }
        }
        StillSyntaxDeclaration::ChoiceType {
            name: maybe_name,
            parameters,
            variants,
        } => {
            highlighted_so_far.push(StillSyntaxNode {
                range: lsp_types::Range {
                    start: still_syntax_declaration_node.range.start,
                    end: lsp_position_add_characters(still_syntax_declaration_node.range.start, 6),
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
            for variant in variants {
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
            type_keyword_range,
            name: maybe_name,
            parameters,
            equals_key_symbol_range: maybe_equals_key_symbol_range,
            type_: maybe_type,
        } => {
            highlighted_so_far.push(StillSyntaxNode {
                range: *type_keyword_range,
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
        StillSyntaxPattern::WithComment {
            comment: comment_node,
            pattern: maybe_pattern_after_comment,
        } => {
            highlighted_so_far.push(StillSyntaxNode {
                range: comment_node.range,
                value: StillSyntaxHighlightKind::Comment,
            });
            if let Some(pattern_node_after_comment) = maybe_pattern_after_comment {
                still_syntax_highlight_pattern_into(
                    highlighted_so_far,
                    still_syntax_node_unbox(pattern_node_after_comment),
                );
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
            inputs,
            arrow_key_symbol_range: maybe_arrow_key_symbol_range,
            output: maybe_output,
        } => {
            for input in inputs {
                still_syntax_highlight_type_into(
                    highlighted_so_far,
                    still_syntax_node_as_ref(input),
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
        StillSyntaxType::WithComment {
            comment: comment_node,
            type_: maybe_type_after_comment,
        } => {
            highlighted_so_far.push(StillSyntaxNode {
                range: comment_node.range,
                value: StillSyntaxHighlightKind::Comment,
            });
            if let Some(type_node_after_comment) = maybe_type_after_comment {
                still_syntax_highlight_type_into(
                    highlighted_so_far,
                    still_syntax_node_unbox(type_node_after_comment),
                );
            }
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
    still_syntax_expression_node: StillSyntaxNode<&StillSyntaxExpression>,
) {
    match still_syntax_expression_node.value {
        StillSyntaxExpression::VariableOrCall {
            variable: variable_node,
            arguments,
        } => {
            highlighted_so_far.push(StillSyntaxNode {
                range: variable_node.range,
                value: StillSyntaxHighlightKind::DeclaredVariable,
            });
            for argument_node in arguments {
                still_syntax_highlight_expression_into(
                    highlighted_so_far,
                    still_syntax_node_as_ref(argument_node),
                );
            }
        }
        StillSyntaxExpression::Match {
            matched: matched_node,
            cases,
        } => {
            still_syntax_highlight_expression_into(
                highlighted_so_far,
                still_syntax_node_unbox(matched_node),
            );
            for case in cases {
                highlighted_so_far.push(StillSyntaxNode {
                    range: case.or_bar_key_symbol_range,
                    value: StillSyntaxHighlightKind::KeySymbol,
                });
                if let Some(case_pattern_node) = &case.pattern {
                    still_syntax_highlight_pattern_into(
                        highlighted_so_far,
                        still_syntax_node_as_ref(case_pattern_node),
                    );
                }
                if let Some(arrow_key_symbol_range) = case.arrow_key_symbol_range {
                    highlighted_so_far.push(StillSyntaxNode {
                        range: arrow_key_symbol_range,
                        value: StillSyntaxHighlightKind::KeySymbol,
                    });
                }
                if let Some(result_node) = &case.result {
                    still_syntax_highlight_expression_into(
                        highlighted_so_far,
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
            parameters,
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
            for parameter_node in parameters {
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
                still_syntax_highlight_expression_into(
                    highlighted_so_far,
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
            if let Some(let_declaration_node) = maybe_declaration {
                still_syntax_highlight_let_declaration_into(
                    highlighted_so_far,
                    still_syntax_node_as_ref(let_declaration_node),
                );
            }
            if let Some(result_node) = maybe_result {
                still_syntax_highlight_expression_into(
                    highlighted_so_far,
                    still_syntax_node_unbox(result_node),
                );
            }
        }
        StillSyntaxExpression::Vec(elements) => {
            for element_node in elements {
                still_syntax_highlight_expression_into(
                    highlighted_so_far,
                    still_syntax_node_as_ref(element_node),
                );
            }
        }
        StillSyntaxExpression::Parenthesized(None) => {}
        StillSyntaxExpression::Parenthesized(Some(in_parens)) => {
            still_syntax_highlight_expression_into(
                highlighted_so_far,
                still_syntax_node_unbox(in_parens),
            );
        }
        StillSyntaxExpression::WithComment {
            comment,
            expression: maybe_expression_after_comment,
        } => {
            highlighted_so_far.push(StillSyntaxNode {
                range: comment.range,
                value: StillSyntaxHighlightKind::KeySymbol,
            });
            if let Some(expression_node_after_comment) = maybe_expression_after_comment {
                still_syntax_highlight_expression_into(
                    highlighted_so_far,
                    still_syntax_node_unbox(expression_node_after_comment),
                );
            }
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
                                still_syntax_node_unbox(value_node),
                            );
                        }
                    }
                    StillSyntaxExpressionUntyped::Other(other_expression_in_typed) => {
                        still_syntax_highlight_expression_into(
                            highlighted_so_far,
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
    still_syntax_let_declaration_node: StillSyntaxNode<&StillSyntaxLetDeclaration>,
) {
    highlighted_so_far.push(StillSyntaxNode {
        range: still_syntax_let_declaration_node.value.name.range,
        value: StillSyntaxHighlightKind::DeclaredVariable,
    });
    if let Some(result_node) = &still_syntax_let_declaration_node.value.result {
        still_syntax_highlight_expression_into(
            highlighted_so_far,
            still_syntax_node_unbox(result_node),
        );
    }
}
// //
struct ParseState<'a> {
    source: &'a str,
    offset_utf8: usize,
    position: lsp_types::Position,
    indent: u16,
    lower_indents_stack: Vec<u16>,
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

fn parse_still_whitespace(state: &mut ParseState) {
    while parse_linebreak(state) || parse_same_line_char_if(state, char::is_whitespace) {}
}
fn parse_still_comment_node(state: &mut ParseState) -> Option<StillSyntaxNode<Box<str>>> {
    let position_before: lsp_types::Position = state.position;
    let content: &str = parse_still_comment(state)?;
    let full_range: lsp_types::Range = lsp_types::Range {
        start: position_before,
        end: state.position,
    };
    Some(StillSyntaxNode {
        range: full_range,
        value: Box::from(content),
    })
}
fn parse_still_comment<'a>(state: &mut ParseState<'a>) -> Option<&'a str> {
    if !parse_symbol(state, "#") {
        return None;
    }
    let content: &str = state.source[state.offset_utf8..]
        .lines()
        .next()
        .unwrap_or("");
    state.offset_utf8 += content.len();
    state.position.character += content.encode_utf16().count() as u32;
    Some(content)
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

fn parse_still_uppercase_name(state: &mut ParseState) -> Option<StillName> {
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
    parse_still_uppercase_name(state).map(|name| StillSyntaxNode {
        range: lsp_types::Range {
            start: start_position,
            end: state.position,
        },
        value: name,
    })
}

fn parse_still_syntax_type(state: &mut ParseState) -> Option<StillSyntaxNode<StillSyntaxType>> {
    parse_still_syntax_type_construct(state)
        .or_else(|| parse_still_syntax_function(state))
        .or_else(|| parse_still_syntax_type_with_comment(state))
        .or_else(|| parse_still_syntax_type_not_space_separated_node(state))
}
fn parse_still_syntax_type_with_comment(
    state: &mut ParseState,
) -> Option<StillSyntaxNode<StillSyntaxType>> {
    let comment_node: StillSyntaxNode<Box<str>> = parse_still_comment_node(state)?;
    parse_still_whitespace(state);
    let maybe_type: Option<StillSyntaxNode<StillSyntaxType>> = parse_still_syntax_type(state);
    Some(StillSyntaxNode {
        range: lsp_types::Range {
            start: comment_node.range.start,
            end: maybe_type
                .as_ref()
                .map(|n| n.range.end)
                .unwrap_or(comment_node.range.end),
        },
        value: StillSyntaxType::WithComment {
            comment: comment_node,
            type_: maybe_type.map(still_syntax_node_box),
        },
    })
}
fn parse_still_syntax_function(state: &mut ParseState) -> Option<StillSyntaxNode<StillSyntaxType>> {
    let backslash_range: lsp_types::Range = parse_symbol_as_range(state, "\\")?;
    parse_still_whitespace(state);
    let mut inputs: Vec<StillSyntaxNode<StillSyntaxType>> = Vec::new();
    while let Some(input_node) = parse_still_syntax_type(state) {
        inputs.push(input_node);
        parse_still_whitespace(state);
        while parse_symbol(state, ",") {
            parse_still_whitespace(state);
        }
    }
    let maybe_arrow_key_symbol_range: Option<lsp_types::Range> = parse_symbol_as_range(state, ">")
        .or_else(|| parse_symbol_as_range(state, "->"))
        .or_else(|| parse_symbol_as_range(state, "=>"));
    parse_still_whitespace(state);
    let maybe_output_type: Option<StillSyntaxNode<StillSyntaxType>> =
        if state.position.character > u32::from(state.indent) {
            parse_still_syntax_type(state)
        } else {
            None
        };
    Some(StillSyntaxNode {
        range: lsp_types::Range {
            start: backslash_range.start,
            end: match &maybe_output_type {
                None => maybe_arrow_key_symbol_range
                    .map(|r| r.end)
                    .or_else(|| inputs.first().map(|n| n.range.end))
                    .unwrap_or(backslash_range.end),
                Some(output_type_node) => output_type_node.range.end,
            },
        },
        value: StillSyntaxType::Function {
            inputs: inputs,
            arrow_key_symbol_range: maybe_arrow_key_symbol_range,
            output: maybe_output_type.map(still_syntax_node_box),
        },
    })
}
fn parse_still_syntax_type_construct(
    state: &mut ParseState,
) -> Option<StillSyntaxNode<StillSyntaxType>> {
    let variable_node: StillSyntaxNode<StillName> = parse_still_lowercase_name_node(state)?;
    parse_still_whitespace(state);
    let mut arguments: Vec<StillSyntaxNode<StillSyntaxType>> = Vec::new();
    let mut construct_end_position: lsp_types::Position = variable_node.range.end;
    while let Some(argument_node) = parse_still_syntax_type_not_space_separated_node(state) {
        construct_end_position = argument_node.range.end;
        arguments.push(argument_node);
        parse_still_whitespace(state);
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
fn parse_still_syntax_type_not_space_separated_node(
    state: &mut ParseState,
) -> Option<StillSyntaxNode<StillSyntaxType>> {
    if state.position.character <= u32::from(state.indent) {
        return None;
    }
    let start_position: lsp_types::Position = state.position;
    let type_: StillSyntaxType = parse_still_uppercase_name(state)
        .map(StillSyntaxType::Variable)
        .or_else(|| parse_still_syntax_type_parenthesized(state))
        .or_else(|| {
            parse_still_lowercase_name_node(state).map(|variable_node| StillSyntaxType::Construct {
                name: variable_node,
                arguments: vec![],
            })
        })
        .or_else(|| parse_still_syntax_type_record(state))?;
    Some(StillSyntaxNode {
        range: lsp_types::Range {
            start: start_position,
            end: state.position,
        },
        value: type_,
    })
}

fn parse_still_syntax_type_record(state: &mut ParseState) -> Option<StillSyntaxType> {
    if !parse_symbol(state, "{") {
        return None;
    }
    parse_still_whitespace(state);
    while parse_symbol(state, ",") {
        parse_still_whitespace(state);
    }
    let mut fields: Vec<StillSyntaxTypeField> = Vec::new();
    while let Some(field) = parse_still_syntax_type_field(state) {
        fields.push(field);
        parse_still_whitespace(state);
        while parse_symbol(state, ",") {
            parse_still_whitespace(state);
        }
    }
    let _: bool = parse_symbol(state, "}");
    Some(StillSyntaxType::Record(fields))
}
fn parse_still_syntax_type_field(state: &mut ParseState) -> Option<StillSyntaxTypeField> {
    if state.position.character <= u32::from(state.indent) {
        return None;
    }
    let name_node: StillSyntaxNode<StillName> = parse_still_lowercase_name_node(state)?;
    parse_still_whitespace(state);
    let maybe_value: Option<StillSyntaxNode<StillSyntaxType>> = parse_still_syntax_type(state);
    Some(StillSyntaxTypeField {
        name: name_node,
        value: maybe_value,
    })
}

fn parse_still_syntax_type_parenthesized(state: &mut ParseState) -> Option<StillSyntaxType> {
    if !parse_symbol(state, "(") {
        return None;
    }
    parse_still_whitespace(state);
    let maybe_in_parens_0: Option<StillSyntaxNode<StillSyntaxType>> =
        parse_still_syntax_type(state);
    parse_still_whitespace(state);
    let _: bool = parse_symbol(state, ")");
    Some(StillSyntaxType::Parenthesized(
        maybe_in_parens_0.map(still_syntax_node_box),
    ))
}

fn parse_still_syntax_pattern(
    state: &mut ParseState,
) -> Option<StillSyntaxNode<StillSyntaxPattern>> {
    if state.position.character < u32::from(state.indent) {
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
        .or_else(|| parse_still_syntax_pattern_with_comment(state))
        .or_else(|| parse_still_syntax_pattern_typed(state))
}
fn parse_still_syntax_pattern_record(state: &mut ParseState) -> Option<StillSyntaxPattern> {
    if !parse_symbol(state, "{") {
        return None;
    }
    parse_still_whitespace(state);
    while parse_symbol(state, ",") {
        parse_still_whitespace(state);
    }
    let mut fields: Vec<StillSyntaxPatternField> = Vec::new();
    while let Some(field_name_node) = if state.position.character <= u32::from(state.indent) {
        None
    } else {
        parse_still_lowercase_name_node(state)
    } {
        parse_still_whitespace(state);
        let maybe_value: Option<StillSyntaxNode<StillSyntaxPattern>> =
            parse_still_syntax_pattern(state);
        fields.push(StillSyntaxPatternField {
            name: field_name_node,
            value: maybe_value,
        });
        parse_still_whitespace(state);
        while parse_symbol(state, ",") {
            parse_still_whitespace(state);
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
    parse_still_whitespace(state);
    let maybe_type: Option<StillSyntaxNode<StillSyntaxType>> = parse_still_syntax_type(state);
    parse_still_whitespace(state);
    let closing_colon_range: Option<lsp_types::Range> = parse_symbol_as_range(state, ":");
    parse_still_whitespace(state);
    let maybe_pattern: Option<StillSyntaxNode<StillSyntaxPatternUntyped>> =
        parse_still_syntax_pattern_untyped(state);
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
fn parse_still_syntax_pattern_untyped(
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
        .or_else(|| parse_still_syntax_pattern_variant(state))
}
fn parse_still_syntax_pattern_variant(
    state: &mut ParseState,
) -> Option<StillSyntaxNode<StillSyntaxPatternUntyped>> {
    let variable_node: StillSyntaxNode<StillName> = parse_still_uppercase_name_node(state)?;
    parse_still_whitespace(state);
    let maybe_value: Option<StillSyntaxNode<StillSyntaxPattern>> =
        parse_still_syntax_pattern(state);
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
fn parse_still_syntax_pattern_with_comment(
    state: &mut ParseState,
) -> Option<StillSyntaxNode<StillSyntaxPattern>> {
    let comment_node: StillSyntaxNode<Box<str>> = parse_still_comment_node(state)?;
    parse_still_whitespace(state);
    let maybe_pattern: Option<StillSyntaxNode<StillSyntaxPattern>> =
        parse_still_syntax_pattern(state);
    Some(StillSyntaxNode {
        range: lsp_types::Range {
            start: comment_node.range.start,
            end: maybe_pattern
                .as_ref()
                .map(|n| n.range.end)
                .unwrap_or(comment_node.range.end),
        },
        value: StillSyntaxPattern::WithComment {
            comment: comment_node,
            pattern: maybe_pattern.map(still_syntax_node_box),
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
    let start_offset_utf8: usize = state.offset_utf8;
    if parse_unsigned_integer_base10(state) {
    } else if parse_symbol(state, "-") || parse_symbol(state, "+") {
        let _: bool = parse_unsigned_integer_base10(state);
    } else {
        return None;
    }
    let decimal_str: &str = &state.source[start_offset_utf8..state.offset_utf8];
    Some(StillSyntaxPattern::Int(Box::from(decimal_str)))
}
fn parse_still_syntax_expression_number(state: &mut ParseState) -> Option<StillSyntaxExpression> {
    let start_offset_utf8: usize = state.offset_utf8;
    if parse_unsigned_integer_base10(state) {
    } else if parse_symbol(state, "-") || parse_symbol(state, "+") {
        let _: bool = parse_unsigned_integer_base10(state);
    } else {
        return None;
    }
    let has_decimal_point: bool = parse_symbol(state, ".");
    if has_decimal_point {
        parse_same_line_while(state, |c| c.is_ascii_digit());
    }
    let full_chomped_str: &str = &state.source[start_offset_utf8..state.offset_utf8];
    Some(if has_decimal_point {
        StillSyntaxExpression::Dec(Box::from(full_chomped_str))
    } else {
        StillSyntaxExpression::Int(Box::from(full_chomped_str))
    })
}
fn parse_still_char(state: &mut ParseState) -> Option<Option<char>> {
    if !parse_symbol(state, "'") {
        return None;
    }
    if parse_symbol(state, "'") {
        return Some(None);
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
        .or_else(|| parse_symbol_as(state, "\\n", '\n'))
        .or_else(|| parse_symbol_as(state, "\\r", '\r'))
        .or_else(|| parse_symbol_as(state, "\\t", '\t'))
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

fn parse_still_syntax_expression_space_separated(
    state: &mut ParseState,
) -> Option<StillSyntaxNode<StillSyntaxExpression>> {
    let start_expression_node: StillSyntaxNode<StillSyntaxExpression> =
        parse_still_syntax_expression_typed(state)
            .or_else(|| parse_still_syntax_expression_let_in(state))
            .or_else(|| parse_still_syntax_expression_lambda(state))
            .or_else(|| parse_still_syntax_expression_variable_or_call(state))
            .or_else(|| parse_still_syntax_expression_with_comment_node(state))
            .or_else(|| parse_still_syntax_expression_not_space_separated(state))?;
    parse_still_whitespace(state);
    let mut cases: Vec<StillSyntaxExpressionCase> = Vec::new();
    'parsing_cases: while let Some((case, is_last_case)) = parse_still_syntax_expression_case(state)
    {
        cases.push(case);
        if is_last_case {
            break 'parsing_cases;
        }
        parse_still_whitespace(state);
    }
    if cases.is_empty() {
        Some(start_expression_node)
    } else {
        Some(StillSyntaxNode {
            range: lsp_types::Range {
                start: start_expression_node.range.start,
                end: cases
                    .last()
                    .map(|last_case| {
                        last_case
                            .result
                            .as_ref()
                            .map(|result| result.range.end)
                            .or_else(|| {
                                last_case
                                    .arrow_key_symbol_range
                                    .as_ref()
                                    .map(|range| range.end)
                            })
                            .or_else(|| last_case.pattern.as_ref().map(|n| n.range.end))
                            .unwrap_or(last_case.or_bar_key_symbol_range.end)
                    })
                    .unwrap_or(start_expression_node.range.end),
            },
            value: StillSyntaxExpression::Match {
                matched: still_syntax_node_box(start_expression_node),
                cases,
            },
        })
    }
}
fn parse_still_syntax_expression_untyped_node(
    state: &mut ParseState,
) -> Option<StillSyntaxNode<StillSyntaxExpressionUntyped>> {
    parse_still_syntax_expression_variant_node(state).or_else(|| {
        parse_still_syntax_expression_space_separated(state).map(|n| StillSyntaxNode {
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
    parse_still_whitespace(state);
    let maybe_type: Option<StillSyntaxNode<StillSyntaxType>> = parse_still_syntax_type(state);
    parse_still_whitespace(state);
    let closing_colon_range: Option<lsp_types::Range> = parse_symbol_as_range(state, ":");
    parse_still_whitespace(state);
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
fn parse_still_syntax_expression_variable_or_call(
    state: &mut ParseState,
) -> Option<StillSyntaxNode<StillSyntaxExpression>> {
    let variable_node: StillSyntaxNode<StillName> =
        parse_still_syntax_expression_variable_standalone(state)?;
    parse_still_whitespace(state);
    let mut arguments: Vec<StillSyntaxNode<StillSyntaxExpression>> = Vec::new();
    let mut call_end_position: lsp_types::Position = variable_node.range.end;
    'parsing_arguments: loop {
        if state.position.character <= u32::from(state.indent) {
            break 'parsing_arguments;
        }
        match parse_still_syntax_expression_not_space_separated(state) {
            None => {
                break 'parsing_arguments;
            }
            Some(argument_node) => {
                call_end_position = argument_node.range.end;
                arguments.push(argument_node);
                parse_still_whitespace(state);
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
    parse_still_whitespace(state);
    let maybe_value: Option<StillSyntaxNode<StillSyntaxExpression>> =
        parse_still_syntax_expression_space_separated(state);
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
fn parse_still_syntax_expression_with_comment_node(
    state: &mut ParseState,
) -> Option<StillSyntaxNode<StillSyntaxExpression>> {
    let comment_node: StillSyntaxNode<Box<str>> = parse_still_comment_node(state)?;
    parse_still_whitespace(state);
    let maybe_expression: Option<StillSyntaxNode<StillSyntaxExpression>> =
        parse_still_syntax_expression_space_separated(state);
    Some(StillSyntaxNode {
        range: lsp_types::Range {
            start: comment_node.range.start,
            end: maybe_expression
                .as_ref()
                .map(|n| n.range.end)
                .unwrap_or(comment_node.range.end),
        },
        value: StillSyntaxExpression::WithComment {
            comment: comment_node,
            expression: maybe_expression.map(still_syntax_node_box),
        },
    })
}
fn parse_still_syntax_expression_not_space_separated(
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
fn parse_still_syntax_expression_variable_standalone(
    state: &mut ParseState,
) -> Option<StillSyntaxNode<StillName>> {
    // can be optimized by e.g. adding a non-state-mutating parse_still_lowercase_as_string
    // that checks for keywords on successful chomp and returns None only then (and if no keyword, mutate the state)
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
    parse_still_whitespace(state);
    if let Some(spread_key_symbol_range) = parse_symbol_as_range(state, "..") {
        parse_still_whitespace(state);
        let maybe_record: Option<StillSyntaxNode<StillSyntaxExpression>> =
            parse_still_syntax_expression_space_separated(state);
        parse_still_whitespace(state);
        while parse_symbol(state, ",") {
            parse_still_whitespace(state);
        }
        let mut fields: Vec<StillSyntaxExpressionField> = Vec::new();
        while let Some(field) = parse_still_syntax_expression_field(state) {
            fields.push(field);
            parse_still_whitespace(state);
            while parse_symbol(state, ",") {
                parse_still_whitespace(state);
            }
        }
        let _: bool = parse_symbol(state, "}");
        Some(StillSyntaxExpression::RecordUpdate {
            record: maybe_record.map(still_syntax_node_box),
            spread_key_symbol_range,
            fields: fields,
        })
    } else {
        while parse_symbol(state, ",") {
            parse_still_whitespace(state);
        }
        let mut fields: Vec<StillSyntaxExpressionField> = Vec::new();
        while let Some(field) = parse_still_syntax_expression_field(state) {
            fields.push(field);
            parse_still_whitespace(state);
            while parse_symbol(state, ",") {
                parse_still_whitespace(state);
            }
        }
        let _: bool = parse_symbol(state, "}");
        Some(StillSyntaxExpression::Record(fields))
    }
}
fn parse_still_syntax_expression_field(
    state: &mut ParseState,
) -> Option<StillSyntaxExpressionField> {
    if state.position.character <= u32::from(state.indent) {
        return None;
    }
    let name_node: StillSyntaxNode<StillName> = parse_still_lowercase_name_node(state)?;
    parse_still_whitespace(state);
    let maybe_value: Option<StillSyntaxNode<StillSyntaxExpression>> =
        parse_still_syntax_expression_space_separated(state);
    Some(StillSyntaxExpressionField {
        name: name_node,
        value: maybe_value,
    })
}
fn parse_still_syntax_expression_lambda(
    state: &mut ParseState,
) -> Option<StillSyntaxNode<StillSyntaxExpression>> {
    let backslash_key_symbol_range: lsp_types::Range = parse_symbol_as_range(state, "\\")?;
    parse_still_whitespace(state);
    let mut parameters: Vec<StillSyntaxNode<StillSyntaxPattern>> = Vec::new();
    while let Some(parameter_node) = parse_still_syntax_pattern(state) {
        parameters.push(parameter_node);
        parse_still_whitespace(state);
        // be lenient in allowing , after lambda parameters, even though it's invalid syntax
        while parse_symbol(state, ",") {
            parse_still_whitespace(state);
        }
    }
    let maybe_arrow_key_symbol_range: Option<lsp_types::Range> = parse_symbol_as_range(state, ">")
        .or_else(|| parse_symbol_as_range(state, "->"))
        .or_else(|| parse_symbol_as_range(state, "=>"));
    parse_still_whitespace(state);
    let maybe_result: Option<StillSyntaxNode<StillSyntaxExpression>> =
        if state.position.character > u32::from(state.indent) {
            parse_still_syntax_expression_space_separated(state)
        } else {
            None
        };
    Some(StillSyntaxNode {
        range: lsp_types::Range {
            start: backslash_key_symbol_range.start,
            end: match &maybe_result {
                None => maybe_arrow_key_symbol_range
                    .map(|r| r.end)
                    .or_else(|| parameters.first().map(|n| n.range.end))
                    .unwrap_or(backslash_key_symbol_range.end),
                Some(result_node) => result_node.range.end,
            },
        },
        value: StillSyntaxExpression::Lambda {
            parameters,
            arrow_key_symbol_range: maybe_arrow_key_symbol_range,
            result: maybe_result.map(still_syntax_node_box),
        },
    })
}
/// second tuple part signifies wether the parsed case must be the last case (TODO make struct)
fn parse_still_syntax_expression_case(
    state: &mut ParseState,
) -> Option<(StillSyntaxExpressionCase, bool)> {
    if state.position.character <= u32::from(state.indent) {
        return None;
    }
    let bar_key_symbol_range: lsp_types::Range = parse_symbol_as_range(state, "|")?;
    parse_still_whitespace(state);
    let maybe_case_pattern: Option<StillSyntaxNode<StillSyntaxPattern>> =
        parse_still_syntax_pattern(state);
    parse_still_whitespace(state);
    match parse_symbol_as_range(state, ">")
        .or_else(|| parse_symbol_as_range(state, "->"))
        .or_else(|| parse_symbol_as_range(state, "=>"))
    {
        None => Some((
            StillSyntaxExpressionCase {
                or_bar_key_symbol_range: bar_key_symbol_range,
                pattern: maybe_case_pattern,
                arrow_key_symbol_range: None,
                result: None,
            },
            false,
        )),
        Some(arrow_key_symbol_range) => {
            parse_still_whitespace(state);
            if state.position.character <= u32::from(state.indent) {
                let maybe_result: Option<StillSyntaxNode<StillSyntaxExpression>> =
                    parse_still_syntax_expression_space_separated(state);
                Some((
                    StillSyntaxExpressionCase {
                        or_bar_key_symbol_range: bar_key_symbol_range,
                        pattern: maybe_case_pattern,
                        arrow_key_symbol_range: Some(arrow_key_symbol_range),
                        result: maybe_result,
                    },
                    true,
                ))
            } else {
                parse_state_push_indent(state, bar_key_symbol_range.start.character as u16);
                let maybe_result: Option<StillSyntaxNode<StillSyntaxExpression>> =
                    parse_still_syntax_expression_space_separated(state);
                parse_state_pop_indent(state);
                Some((
                    StillSyntaxExpressionCase {
                        or_bar_key_symbol_range: bar_key_symbol_range,
                        pattern: maybe_case_pattern,
                        arrow_key_symbol_range: Some(arrow_key_symbol_range),
                        result: maybe_result,
                    },
                    false,
                ))
            }
        }
    }
}

fn parse_still_syntax_expression_let_in(
    state: &mut ParseState,
) -> Option<StillSyntaxNode<StillSyntaxExpression>> {
    let let_keyword_range: lsp_types::Range = parse_still_keyword_as_range(state, "let")?;
    parse_still_whitespace(state);
    Some(if state.position.character <= u32::from(state.indent) {
        StillSyntaxNode {
            range: let_keyword_range,
            value: StillSyntaxExpression::Let {
                declaration: None,
                result: None,
            },
        }
    } else {
        parse_state_push_indent(state, let_keyword_range.start.character as u16);
        let mut syntax_before_in_key_symbol_end_position: lsp_types::Position =
            let_keyword_range.end;
        let maybe_declaration: Option<StillSyntaxNode<StillSyntaxLetDeclaration>> =
            parse_still_syntax_let_declaration(state);
        if let Some(declaration_node) = &maybe_declaration {
            syntax_before_in_key_symbol_end_position = declaration_node.range.end;
            parse_still_whitespace(state);
        }
        parse_state_pop_indent(state);
        parse_still_whitespace(state);
        let maybe_result: Option<StillSyntaxNode<StillSyntaxExpression>> =
            parse_still_syntax_expression_space_separated(state);
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
    let name_node: StillSyntaxNode<StillName> = parse_still_lowercase_name_node(state)?;
    parse_still_whitespace(state);
    let _: bool = parse_symbol(state, "=");
    parse_still_whitespace(state);
    let maybe_result: Option<StillSyntaxNode<StillSyntaxExpression>> =
        if state.position.character <= u32::from(state.indent) {
            None
        } else {
            parse_still_syntax_expression_space_separated(state)
        };
    Some(StillSyntaxNode {
        range: lsp_types::Range {
            start: name_node.range.start,
            end: maybe_result
                .as_ref()
                .map(|node| node.range.end)
                .unwrap_or(name_node.range.end),
        },
        value: StillSyntaxLetDeclaration {
            name: name_node,
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
    parse_still_whitespace(state);
    while parse_symbol(state, ",") {
        parse_still_whitespace(state);
    }
    let mut elements: Vec<StillSyntaxNode<StillSyntaxExpression>> = Vec::new();
    while let Some(expression_node) = parse_still_syntax_expression_space_separated(state) {
        elements.push(expression_node);
        parse_still_whitespace(state);
        while parse_symbol(state, ",") {
            parse_still_whitespace(state);
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
    parse_still_whitespace(state);
    let maybe_in_parens_0: Option<StillSyntaxNode<StillSyntaxExpression>> =
        parse_still_syntax_expression_space_separated(state);
    parse_still_whitespace(state);
    let _ = parse_symbol(state, ")");
    Some(StillSyntaxExpression::Parenthesized(
        maybe_in_parens_0.map(still_syntax_node_box),
    ))
}
fn parse_still_syntax_declaration_node(
    state: &mut ParseState,
) -> Option<StillSyntaxNode<StillSyntaxDeclaration>> {
    parse_still_syntax_declaration_choice_type_node(state)
        .or_else(|| parse_still_syntax_declaration_type_alias_node(state))
        .or_else(|| {
            if state.indent != 0 {
                return None;
            }
            parse_still_syntax_declaration_variable_node(state)
        })
}
fn parse_still_syntax_declaration_type_alias_node(
    state: &mut ParseState,
) -> Option<StillSyntaxNode<StillSyntaxDeclaration>> {
    let type_keyword_range: lsp_types::Range = parse_still_keyword_as_range(state, "type")?;
    parse_still_whitespace(state);
    let maybe_name_node: Option<StillSyntaxNode<StillName>> =
        parse_still_lowercase_name_node(state);
    parse_still_whitespace(state);
    let mut parameters: Vec<StillSyntaxNode<StillName>> = Vec::new();
    while let Some(parameter_node) = parse_still_uppercase_name_node(state) {
        parameters.push(parameter_node);
        parse_still_whitespace(state);
    }
    let maybe_equals_key_symbol_range: Option<lsp_types::Range> = parse_symbol_as_range(state, "=");
    parse_still_whitespace(state);
    let maybe_type: Option<StillSyntaxNode<StillSyntaxType>> =
        if state.position.character <= u32::from(state.indent) {
            None
        } else {
            parse_still_syntax_type(state)
        };
    let full_end_location: lsp_types::Position = maybe_type
        .as_ref()
        .map(|type_node| type_node.range.end)
        .or_else(|| maybe_equals_key_symbol_range.map(|range| range.end))
        .or_else(|| parameters.last().map(|n| n.range.end))
        .or_else(|| {
            maybe_name_node
                .as_ref()
                .map(|name_node| name_node.range.end)
        })
        .unwrap_or(type_keyword_range.end);
    Some(StillSyntaxNode {
        range: lsp_types::Range {
            start: type_keyword_range.start,
            end: full_end_location,
        },
        value: StillSyntaxDeclaration::TypeAlias {
            type_keyword_range: type_keyword_range,
            name: maybe_name_node,
            parameters: parameters,
            equals_key_symbol_range: maybe_equals_key_symbol_range,
            type_: maybe_type,
        },
    })
}
fn parse_still_syntax_declaration_choice_type_node(
    state: &mut ParseState,
) -> Option<StillSyntaxNode<StillSyntaxDeclaration>> {
    let choice_keyword_range: lsp_types::Range = parse_still_keyword_as_range(state, "choice")?;
    parse_still_whitespace(state);
    let maybe_name_node: Option<StillSyntaxNode<StillName>> =
        parse_still_lowercase_name_node(state);
    parse_still_whitespace(state);
    let mut parameters: Vec<StillSyntaxNode<StillName>> = Vec::new();
    while let Some(parameter_node) = parse_still_uppercase_name_node(state) {
        parameters.push(parameter_node);
        parse_still_whitespace(state);
    }
    let maybe_equals_key_symbol_range: Option<lsp_types::Range> = parse_symbol_as_range(state, "=");
    parse_still_whitespace(state);

    let mut variants: Vec<StillSyntaxChoiceTypeVariant> = Vec::new();
    while let Some(variant) = parse_still_syntax_choice_type_declaration_variant(state) {
        variants.push(variant);
        parse_still_whitespace(state);
    }
    Some(StillSyntaxNode {
        range: lsp_types::Range {
            start: choice_keyword_range.start,
            end: variants
                .last()
                .map(|variant| {
                    variant
                        .value
                        .as_ref()
                        .map(|n| n.range.end)
                        .or_else(|| variant.name.as_ref().map(|node| node.range.end))
                        .unwrap_or(variant.or_key_symbol_range.end)
                })
                .or_else(|| maybe_equals_key_symbol_range.map(|r| r.end))
                .or_else(|| parameters.last().map(|n| n.range.end))
                .or_else(|| {
                    maybe_name_node
                        .as_ref()
                        .map(|name_node| name_node.range.end)
                })
                .unwrap_or(choice_keyword_range.end),
        },
        value: StillSyntaxDeclaration::ChoiceType {
            name: maybe_name_node,
            parameters: parameters,

            variants,
        },
    })
}
fn parse_still_syntax_choice_type_declaration_variant(
    state: &mut ParseState,
) -> Option<StillSyntaxChoiceTypeVariant> {
    let or_key_symbol_range: lsp_types::Range = parse_symbol_as_range(state, "|")?;
    parse_still_whitespace(state);
    while parse_symbol(state, "|") {
        parse_still_whitespace(state);
    }
    let maybe_name: Option<StillSyntaxNode<StillName>> = parse_still_uppercase_name_node(state);
    parse_still_whitespace(state);
    let maybe_value: Option<StillSyntaxNode<StillSyntaxType>> = parse_still_syntax_type(state);
    parse_still_whitespace(state);
    Some(StillSyntaxChoiceTypeVariant {
        or_key_symbol_range: or_key_symbol_range,
        name: maybe_name,
        value: maybe_value,
    })
}
fn parse_still_syntax_declaration_variable_node(
    state: &mut ParseState,
) -> Option<StillSyntaxNode<StillSyntaxDeclaration>> {
    let name_node: StillSyntaxNode<StillName> = parse_still_lowercase_name_node(state)?;
    parse_still_whitespace(state);
    let _: bool = parse_symbol(state, "=");
    parse_still_whitespace(state);
    let maybe_result: Option<StillSyntaxNode<StillSyntaxExpression>> =
        if state.position.character <= u32::from(state.indent) {
            None
        } else {
            parse_still_syntax_expression_space_separated(state)
        };
    Some(StillSyntaxNode {
        range: lsp_types::Range {
            start: name_node.range.start,
            end: maybe_result
                .as_ref()
                .map(|node| node.range.end)
                .unwrap_or(name_node.range.end),
        },
        value: StillSyntaxDeclaration::Variable {
            name: name_node,
            result: maybe_result,
        },
    })
}
fn parse_still_syntax_documented_declaration_followed_by_whitespace_and_whatever_indented(
    state: &mut ParseState,
) -> Option<StillSyntaxDocumentedDeclaration> {
    let start_position: lsp_types::Position = state.position;
    let maybe_documentation_node = parse_still_comment(state).map(|first_comment_line| {
        let mut full_comment_content: String = first_comment_line.to_string();
        let mut end_position: lsp_types::Position = state.position;
        parse_still_whitespace(state);
        while let Some(next_comment_line) = parse_still_comment(state) {
            full_comment_content.push('\n');
            full_comment_content.push_str(next_comment_line);
            end_position = state.position;
            parse_still_whitespace(state);
        }
        StillSyntaxNode {
            range: lsp_types::Range {
                start: start_position,
                end: end_position,
            },
            value: full_comment_content.into_boxed_str(),
        }
    });
    match maybe_documentation_node {
        None => parse_still_syntax_declaration_node(state).map(|declaration_node| {
            parse_still_whitespace(state);
            StillSyntaxDocumentedDeclaration {
                documentation: None,
                declaration: Some(declaration_node),
            }
        }),
        Some(documentation_node) => {
            parse_still_whitespace(state);
            let maybe_declaration: Option<StillSyntaxNode<StillSyntaxDeclaration>> =
                parse_still_syntax_declaration_node(state);
            parse_still_whitespace(state);
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
    };
    parse_still_whitespace(&mut state);
    let mut last_valid_end_offset_utf8: usize = state.offset_utf8;
    let mut last_valid_end_position: lsp_types::Position = state.position;
    let mut last_parsed_was_valid: bool = true;
    let mut declarations: Vec<Result<StillSyntaxDocumentedDeclaration, StillSyntaxNode<Box<str>>>> =
        Vec::with_capacity(8);
    'parsing_declarations: loop {
        let offset_utf8_before_parsing_documented_declaration: usize = state.offset_utf8;
        let position_before_parsing_documented_declaration: lsp_types::Position = state.position;
        match parse_still_syntax_documented_declaration_followed_by_whitespace_and_whatever_indented(
            &mut state,
        ) {
            Some(documented_declaration) => {
                if !last_parsed_was_valid {
                    declarations.push(Err(StillSyntaxNode {
                        range: lsp_types::Range {
                            start: last_valid_end_position,
                            end: position_before_parsing_documented_declaration,
                        },
                        value: Box::from(
                            &project_source[last_valid_end_offset_utf8
                                ..offset_utf8_before_parsing_documented_declaration],
                        ),
                    }));
                }
                last_parsed_was_valid = true;
                declarations.push(Ok(documented_declaration));
                parse_still_whitespace(&mut state);
                last_valid_end_offset_utf8 = state.offset_utf8;
                last_valid_end_position = state.position;
            }
            None => {
                if state.offset_utf8 >= state.source.len() {
                    break 'parsing_declarations;
                }
                last_parsed_was_valid = false;
                parse_before_next_linebreak(&mut state);
                if !parse_linebreak(&mut state) {
                    break 'parsing_declarations;
                }
            }
        }
    }
    if !last_parsed_was_valid {
        let unknown_source: &str = &project_source[last_valid_end_offset_utf8..];
        let mut unknown_source_lines_iterator_rev = unknown_source.lines().rev();
        let end_position: lsp_types::Position = match unknown_source_lines_iterator_rev.next() {
            None => lsp_position_add_characters(
                last_valid_end_position,
                unknown_source.encode_utf16().count() as i32,
            ),
            Some(last_unknown_line) => {
                let unknown_line_count: usize = 1 + unknown_source_lines_iterator_rev.count();
                lsp_types::Position {
                    line: last_valid_end_position.line + unknown_line_count as u32 - 1,
                    character: last_unknown_line.encode_utf16().count() as u32,
                }
            }
        };
        declarations.push(Err(StillSyntaxNode {
            range: lsp_types::Range {
                start: last_valid_end_position,
                end: end_position,
            },
            value: Box::from(unknown_source),
        }));
    }
    StillSyntaxProject {
        declarations: declarations,
    }
}

#[derive(Clone, Copy)]
struct StillSyntaxVariableDeclarationInfo<'a> {
    range: lsp_types::Range,
    documentation: Option<&'a StillSyntaxNode<Box<str>>>,
    name: &'a StillSyntaxNode<StillName>,
    result: Option<StillSyntaxNode<&'a StillSyntaxExpression>>,
}
#[derive(Clone, Copy)]
enum StillSyntaxTypeDeclarationInfo<'a> {
    // consider introducing separate structs instead of separately referencing each field
    ChoiceType {
        documentation: &'a Option<StillSyntaxNode<Box<str>>>,
        name: &'a StillSyntaxNode<StillName>,
        parameters: &'a Vec<StillSyntaxNode<StillName>>,
        variants: &'a Vec<StillSyntaxChoiceTypeVariant>,
    },
    TypeAlias {
        documentation: &'a Option<StillSyntaxNode<Box<str>>>,
        name: &'a StillSyntaxNode<StillName>,
        parameters: &'a Vec<StillSyntaxNode<StillName>>,
        type_: &'a Option<StillSyntaxNode<StillSyntaxType>>,
    },
}
fn still_project_compile_to_rust(
    errors: &mut Vec<StillErrorNode>,
    StillSyntaxProject { declarations }: &StillSyntaxProject,
) -> CompiledProject {
    let mut type_graph: strongly_connected_components::Graph =
        strongly_connected_components::Graph::new();
    let mut type_graph_node_by_name: std::collections::HashMap<
        &str,
        strongly_connected_components::Node,
    > = std::collections::HashMap::new();
    let mut type_declaration_by_graph_node: std::collections::HashMap<
        strongly_connected_components::Node,
        StillSyntaxTypeDeclarationInfo,
    > = std::collections::HashMap::new();

    let mut variable_graph: strongly_connected_components::Graph =
        strongly_connected_components::Graph::new();
    let mut variable_graph_node_by_name: std::collections::HashMap<
        &str,
        strongly_connected_components::Node,
    > = std::collections::HashMap::with_capacity(declarations.len());
    let mut variable_declaration_by_graph_node: std::collections::HashMap<
        strongly_connected_components::Node,
        StillSyntaxVariableDeclarationInfo,
    > = std::collections::HashMap::with_capacity(declarations.len());

    for declaration_node_or_err in declarations {
        match declaration_node_or_err {
            Err(unknown_node) => {
                errors.push(StillErrorNode {
                    range: unknown_node.range,
                    message: Box::from(if unknown_node.value.starts_with('_') {
                        "unrecognized syntax. Identifiers consist of ascii letters, digits and -"
                    } else   if unknown_node
                        .value
                        .starts_with(|c: char| c.is_ascii_lowercase())
                    {
                        "unrecognized syntax. It could be that an uppercase letter is expected here. Also, is it indented correctly?"
                    } else if unknown_node
                        .value
                        .starts_with(|c: char| c.is_ascii_uppercase())
                    {
                        "unrecognized syntax. It could be that a lowercase letter is expected here. Also, is it indented correctly?"
                    } else {
                        "unrecognized syntax. Is it indented correctly?"
                    }),
                });
            }
            Ok(documented_declaration) => {
                if let Some(declaration_node) = &documented_declaration.declaration {
                    match &declaration_node.value {
                        StillSyntaxDeclaration::ChoiceType {
                            name: maybe_name,
                            parameters,
                            variants,
                        } => match maybe_name {
                            None => {
                                errors.push(StillErrorNode { range: declaration_node.range, message: Box::from("missing name. Type names start with a lowercase letter any only use ascii alphanumeric characters and -)") });
                            }
                            Some(name_node) => {
                                let choice_type_declaration_graph_node: strongly_connected_components::Node =
                                type_graph.new_node();
                                type_graph_node_by_name
                                    .insert(&name_node.value, choice_type_declaration_graph_node);
                                let existing_type_with_same_name: Option<
                                    StillSyntaxTypeDeclarationInfo,
                                > = type_declaration_by_graph_node.insert(
                                    choice_type_declaration_graph_node,
                                    StillSyntaxTypeDeclarationInfo::ChoiceType {
                                        documentation: &documented_declaration.documentation,
                                        name: name_node,
                                        parameters: parameters,
                                        variants,
                                    },
                                );
                                if existing_type_with_same_name.is_some() {
                                    errors.push(StillErrorNode {
                                        range: name_node.range,
                                        message: Box::from("a type with this name is already declared. Rename one of them")
                                    });
                                } else if core_choice_type_infos.contains_key(&name_node.value) {
                                    errors.push(StillErrorNode {
                                        range: name_node.range,
                                        message: Box::from("a type with this name is already part of core (core types are for example vec, int, str). Rename this type")
                                    });
                                }
                            }
                        },
                        StillSyntaxDeclaration::TypeAlias {
                            type_keyword_range: _,
                            name: maybe_name,
                            parameters,
                            equals_key_symbol_range: _,
                            type_: maybe_type,
                        } => match maybe_name {
                            None => {
                                errors.push(StillErrorNode { range: declaration_node.range, message: Box::from("missing name. Type names start with a lowercase letter any only use ascii alphanumeric characters and -)") });
                            }
                            Some(name_node) => {
                                let type_alias_declaration_graph_node: strongly_connected_components::Node =
                                type_graph.new_node();
                                type_graph_node_by_name
                                    .insert(&name_node.value, type_alias_declaration_graph_node);
                                let existing_type_with_same_name: Option<
                                    StillSyntaxTypeDeclarationInfo,
                                > = type_declaration_by_graph_node.insert(
                                    type_alias_declaration_graph_node,
                                    StillSyntaxTypeDeclarationInfo::TypeAlias {
                                        documentation: &documented_declaration.documentation,
                                        name: name_node,
                                        parameters: parameters,
                                        type_: maybe_type,
                                    },
                                );
                                if existing_type_with_same_name.is_some() {
                                    errors.push(StillErrorNode {
                                        range: name_node.range,
                                        message: Box::from("a type with this name is already declared. Rename one of them")
                                    });
                                }
                            }
                        },
                        StillSyntaxDeclaration::Variable {
                            name: name_node,
                            result: maybe_result,
                        } => {
                            let variable_declaration_graph_node: strongly_connected_components::Node =
                            variable_graph.new_node();
                            variable_graph_node_by_name
                                .insert(&name_node.value, variable_declaration_graph_node);
                            let existing_variable_with_same_name: Option<
                                StillSyntaxVariableDeclarationInfo,
                            > = variable_declaration_by_graph_node.insert(
                                variable_declaration_graph_node,
                                StillSyntaxVariableDeclarationInfo {
                                    range: declaration_node.range,
                                    documentation: documented_declaration.documentation.as_ref(),
                                    name: name_node,
                                    result: maybe_result.as_ref().map(still_syntax_node_as_ref),
                                },
                            );
                            if existing_variable_with_same_name.is_some() {
                                errors.push(StillErrorNode {
                                    range: name_node.range,
                                    message: Box::from("a variable with this name is already declared. Rename one of them")
                                });
                            } else if core_choice_type_infos.contains_key(&name_node.value) {
                                errors.push(StillErrorNode {
                                    range: name_node.range,
                                    message: Box::from("a variable with this name is already part of core (core variables are for example int-to-str or dec-add). Rename this variable")
                                });
                            }
                        }
                    }
                }
            }
        }
    }
    for (&type_declaration_graph_node, &type_declaration_info) in
        type_declaration_by_graph_node.iter()
    {
        still_syntax_type_declaration_connect_type_names_in_graph_from(
            &mut type_graph,
            type_declaration_graph_node,
            &type_graph_node_by_name,
            type_declaration_info,
        );
    }
    for (&variable_declaration_graph_node, &variable_declaration_info) in
        variable_declaration_by_graph_node.iter()
    {
        if let Some(result_node) = variable_declaration_info.result {
            still_syntax_expression_connect_variables_in_graph_from(
                &mut variable_graph,
                variable_declaration_graph_node,
                &variable_graph_node_by_name,
                result_node,
            );
        }
    }
    still_project_info_to_rust(
        errors,
        &type_graph,
        &type_declaration_by_graph_node,
        &variable_graph,
        &variable_declaration_by_graph_node,
    )
}
struct CompiledProject {
    rust: syn::File,
    type_aliases: std::collections::HashMap<StillName, TypeAliasInfo>,
    choice_types: std::collections::HashMap<StillName, ChoiceTypeInfo>,
    variable_declarations: std::collections::HashMap<StillName, CompiledVariableDeclarationInfo>,
}
fn still_project_info_to_rust(
    errors: &mut Vec<StillErrorNode>,
    type_graph: &strongly_connected_components::Graph,
    type_declaration_by_graph_node: &std::collections::HashMap<
        strongly_connected_components::Node,
        StillSyntaxTypeDeclarationInfo,
    >,
    variable_graph: &strongly_connected_components::Graph,
    variable_declaration_by_graph_node: &std::collections::HashMap<
        strongly_connected_components::Node,
        StillSyntaxVariableDeclarationInfo,
    >,
) -> CompiledProject {
    let mut rust_items: Vec<syn::Item> =
        Vec::with_capacity(type_graph.len() * 3 + variable_graph.len());
    let mut compiled_type_alias_infos: std::collections::HashMap<StillName, TypeAliasInfo> =
        std::collections::HashMap::new();
    let mut compiled_choice_type_infos: std::collections::HashMap<StillName, ChoiceTypeInfo> =
        core_choice_type_infos.clone();
    let mut records_used: std::collections::HashSet<Vec<StillName>> =
        std::collections::HashSet::new();
    'compile_types: for type_declaration_strongly_connected_component in
        type_graph.find_sccs().iter_sccs()
    {
        let type_declaration_infos: Vec<StillSyntaxTypeDeclarationInfo> =
            type_declaration_strongly_connected_component
                .iter_nodes()
                .filter_map(|variable_declaration_graph_node| {
                    type_declaration_by_graph_node.get(&variable_declaration_graph_node)
                })
                .copied()
                .collect::<Vec<_>>();
        let mut scc_type_alias_count: usize = 0;
        // initialize only the parameters into compiled_choice_type_infos
        // so that no "not found" errors are raised
        for type_declaration_info in &type_declaration_infos {
            match type_declaration_info {
                StillSyntaxTypeDeclarationInfo::TypeAlias {
                    name: name_node,
                    parameters,
                    ..
                } => {
                    scc_type_alias_count += 1;
                    compiled_type_alias_infos.insert(
                        name_node.value.clone(),
                        TypeAliasInfo {
                            parameters: (*parameters).clone(),
                            name_range: None,
                            documentation: None,
                            type_syntax: None,
                            type_: None,
                            is_copy: false,
                            has_owned_representation: false,
                            has_lifetime_parameter: true,
                        },
                    );
                }
                StillSyntaxTypeDeclarationInfo::ChoiceType {
                    name: name_node,
                    parameters,
                    ..
                } => {
                    compiled_choice_type_infos.insert(
                        name_node.value.clone(),
                        ChoiceTypeInfo {
                            parameters: (*parameters).clone(),
                            name_range: None,
                            documentation: None,
                            variants: vec![],
                            is_copy: false,
                            has_owned_representation: false,
                            has_lifetime_parameter: true,
                            type_variants: vec![],
                        },
                    );
                }
            }
        }
        // report and skip (mutually) recursive type aliases. a bit messy
        if scc_type_alias_count >= 2 {
            let error_message: Box<str> = format!(
                "this type alias is part of multiple (mutually) recursive types, multiple of which type aliases. That means it references type aliases that themselves eventually reference this type alias. The involved types are: {}. While there are legitimate uses for this, it can generally be tricky to represent in compile target languages, and can even lead to the type checker running in circles. You can break this infinite loop by wrapping this type or one of its recursive parts into a choice type. Choice types are allowed to recurse as much as they like.",
                type_declaration_infos
                    .iter()
                    .map(|type_declaration_info| match type_declaration_info {
                        StillSyntaxTypeDeclarationInfo::TypeAlias { name:name_node, .. } => name_node.value.as_str(),
                        StillSyntaxTypeDeclarationInfo::ChoiceType { name:name_node,.. } => name_node.value.as_str(),
                    })
                    .collect::<Vec<&str>>()
                    .join(", ")
                ).into_boxed_str();
            errors.extend(
                type_declaration_infos
                    .iter()
                    .filter_map(
                        |scc_type_declaration_info| match scc_type_declaration_info {
                            StillSyntaxTypeDeclarationInfo::TypeAlias {
                                name: scc_type_alias_name_node,
                                ..
                            } => Some(scc_type_alias_name_node.range),
                            StillSyntaxTypeDeclarationInfo::ChoiceType { .. } => None,
                        },
                    )
                    .map(|scc_type_alias_name_range| StillErrorNode {
                        range: scc_type_alias_name_range,
                        message: error_message.clone(),
                    }),
            );
            continue 'compile_types;
        } else if scc_type_alias_count == 1
            && type_declaration_infos.len() == 1
            && let Some(first_scc_type_node) = type_declaration_strongly_connected_component
                .iter_nodes()
                .next()
            && type_graph
                .iter_successors(first_scc_type_node)
                .any(|n| n == first_scc_type_node)
            && let Some(StillSyntaxTypeDeclarationInfo::TypeAlias {
                name: first_scc_type_declaration_name_node,
                ..
            }) = type_declaration_infos.first()
        {
            errors.push(StillErrorNode {
                    range: first_scc_type_declaration_name_node.range,
                    message: Box::from("this type alias is recursive: it references itself in the type is aliases. This is tricky to represent in compile target languages, and can even lead to the type checker running in circles. You can break this infinite loop by wrapping this type or one of its recursive parts into a choice type."),
                });
            continue 'compile_types;
        }
        let scc_type_declaration_names: std::collections::HashSet<&str> = type_declaration_infos
            .iter()
            .map(|&type_declaration| match type_declaration {
                StillSyntaxTypeDeclarationInfo::ChoiceType { name, .. } => name.value.as_str(),
                StillSyntaxTypeDeclarationInfo::TypeAlias { name, .. } => name.value.as_str(),
            })
            .collect::<std::collections::HashSet<_>>();
        for type_declaration_info in type_declaration_infos {
            match type_declaration_info {
                StillSyntaxTypeDeclarationInfo::TypeAlias {
                    documentation: maybe_documentation,
                    name: name_node,
                    parameters,
                    type_: maybe_type,
                } => {
                    let maybe_compiled_type_alias: Option<CompiledTypeAlias> =
                        type_alias_declaration_to_rust(
                            errors,
                            &mut records_used,
                            &compiled_type_alias_infos,
                            &compiled_choice_type_infos,
                            maybe_documentation.as_ref().map(|n| n.value.as_ref()),
                            still_syntax_node_as_ref(name_node),
                            parameters,
                            maybe_type.as_ref().map(still_syntax_node_as_ref),
                        );
                    if let Some(compiled_type_declaration) = maybe_compiled_type_alias {
                        rust_items.push(compiled_type_declaration.rust);
                        compiled_type_alias_infos.insert(
                            name_node.value.clone(),
                            TypeAliasInfo {
                                name_range: Some(name_node.range),
                                documentation: maybe_documentation
                                    .as_ref()
                                    .map(|n| n.value.clone()),
                                parameters: parameters.clone(),
                                type_syntax: maybe_type.clone(),
                                type_: Some(compiled_type_declaration.type_),
                                is_copy: compiled_type_declaration.is_copy,
                                has_owned_representation: compiled_type_declaration
                                    .has_owned_representation,
                                has_lifetime_parameter: compiled_type_declaration
                                    .has_lifetime_parameter,
                            },
                        );
                    } else {
                        compiled_type_alias_infos.insert(
                            name_node.value.clone(),
                            TypeAliasInfo {
                                name_range: Some(name_node.range),
                                documentation: maybe_documentation
                                    .as_ref()
                                    .map(|n| n.value.clone()),
                                parameters: parameters.clone(),
                                type_syntax: maybe_type.clone(),
                                type_: None,
                                // dummy values that should not be read in practice
                                is_copy: false,
                                has_owned_representation: false,
                                has_lifetime_parameter: true,
                            },
                        );
                    }
                }
                StillSyntaxTypeDeclarationInfo::ChoiceType {
                    documentation: maybe_documentation,
                    name: name_node,
                    parameters,
                    variants,
                } => {
                    let maybe_compiled_choice_type_info: Option<CompiledRustChoiceTypeInfo> =
                        choice_type_declaration_to_rust_into(
                            &mut rust_items,
                            errors,
                            &mut records_used,
                            &compiled_type_alias_infos,
                            &compiled_choice_type_infos,
                            &scc_type_declaration_names,
                            maybe_documentation.as_ref().map(|n| n.value.as_ref()),
                            still_syntax_node_as_ref(name_node),
                            parameters,
                            variants,
                        );
                    let info: ChoiceTypeInfo = match maybe_compiled_choice_type_info {
                        Some(compiled_choice_type_info) => ChoiceTypeInfo {
                            name_range: Some(name_node.range),
                            documentation: maybe_documentation.as_ref().map(|n| n.value.clone()),
                            parameters: parameters.clone(),
                            variants: variants.clone(),
                            is_copy: compiled_choice_type_info.is_copy,
                            has_owned_representation: compiled_choice_type_info
                                .has_owned_representation,
                            has_lifetime_parameter: compiled_choice_type_info
                                .has_lifetime_parameter,
                            type_variants: compiled_choice_type_info.variants,
                        },
                        None => ChoiceTypeInfo {
                            name_range: Some(name_node.range),
                            documentation: maybe_documentation.as_ref().map(|n| n.value.clone()),
                            parameters: parameters.clone(),
                            variants: variants.clone(),
                            // dummy
                            is_copy: false,
                            has_owned_representation: false,
                            has_lifetime_parameter: true,
                            type_variants: vec![],
                        },
                    };
                    compiled_choice_type_infos.insert(name_node.value.clone(), info);
                }
            }
        }
    }
    let mut compiled_variable_declaration_infos: std::collections::HashMap<
        StillName,
        CompiledVariableDeclarationInfo,
    > = core_variable_declaration_infos.clone();
    compiled_variable_declaration_infos.reserve(variable_graph.len());
    for variable_declaration_strongly_connected_component in variable_graph.find_sccs().iter_sccs()
    {
        let variable_declarations_in_strongly_connected_component: Vec<
            StillSyntaxVariableDeclarationInfo,
        > = variable_declaration_strongly_connected_component
            .iter_nodes()
            .filter_map(|variable_declaration_graph_node| {
                variable_declaration_by_graph_node.get(&variable_declaration_graph_node)
            })
            .copied()
            .collect();
        for variable_declaration in &variable_declarations_in_strongly_connected_component {
            match variable_declaration.result {
                None => {
                    compiled_variable_declaration_infos.insert(
                        variable_declaration.name.value.clone(),
                        CompiledVariableDeclarationInfo {
                            name_range: Some(variable_declaration.name.range),
                            documentation: variable_declaration
                                .documentation
                                .map(|n| n.value.clone()),
                            type_: None,
                            has_allocator_parameter: true,
                            kind: RustVariableItemKind::Fn,
                        },
                    );
                }
                Some(result_node) => {
                    let result_type_node: StillSyntaxNode<StillSyntaxType> =
                        still_syntax_expression_type(
                            &compiled_type_alias_infos,
                            &compiled_variable_declaration_infos,
                            result_node,
                        );
                    compiled_variable_declaration_infos.insert(
                        variable_declaration.name.value.clone(),
                        CompiledVariableDeclarationInfo {
                            name_range: Some(variable_declaration.range),
                            documentation: variable_declaration
                                .documentation
                                .map(|n| n.value.clone()),
                            type_: still_syntax_type_to_type(
                                // will be reported when compiling
                                &mut Vec::new(),
                                &compiled_type_alias_infos,
                                &compiled_choice_type_infos,
                                still_syntax_node_as_ref(&result_type_node),
                            ),
                            kind: RustVariableItemKind::Fn,
                            has_allocator_parameter: true,
                        },
                    );
                }
            }
        }
        for variable_declaration in variable_declarations_in_strongly_connected_component {
            let maybe_compiled_variable_declaration: Option<CompiledVariableDeclaration> =
                variable_declaration_to_rust(
                    errors,
                    &mut records_used,
                    &compiled_type_alias_infos,
                    &compiled_choice_type_infos,
                    &compiled_variable_declaration_infos,
                    variable_declaration,
                );
            if let Some(compiled_variable_declaration) = maybe_compiled_variable_declaration {
                rust_items.push(compiled_variable_declaration.rust);
                compiled_variable_declaration_infos.insert(
                    variable_declaration.name.value.clone(),
                    CompiledVariableDeclarationInfo {
                        documentation: variable_declaration.documentation.map(|n| n.value.clone()),
                        name_range: Some(variable_declaration.name.range),
                        kind: compiled_variable_declaration.kind,
                        has_allocator_parameter: compiled_variable_declaration
                            .has_allocator_parameter,
                        type_: Some(compiled_variable_declaration.type_),
                    },
                );
            }
        }
    }
    rust_items.reserve(records_used.len());
    for used_still_record_fields in records_used.into_iter().filter(|fields| !fields.is_empty()) {
        rust_items.extend(still_syntax_record_to_rust(&used_still_record_fields));
    }
    CompiledProject {
        rust: syn::File {
            shebang: None,
            attrs: vec![],
            items: rust_items,
        },
        type_aliases: compiled_type_alias_infos,
        choice_types: compiled_choice_type_infos,
        variable_declarations: compiled_variable_declaration_infos,
    }
}
#[derive(Clone)]
struct CompiledVariableDeclarationInfo {
    name_range: Option<lsp_types::Range>,
    documentation: Option<Box<str>>,
    kind: RustVariableItemKind,
    type_: Option<StillType>,
    has_allocator_parameter: bool,
}
static core_variable_declaration_infos: std::sync::LazyLock<
    std::collections::HashMap<StillName, CompiledVariableDeclarationInfo>,
> = {
    fn variable(name: &'static str) -> StillType {
        StillType::Variable(StillName::from(name))
    }
    fn function(inputs: impl IntoIterator<Item = StillType>, output: StillType) -> StillType {
        StillType::Function {
            inputs: inputs.into_iter().collect::<Vec<_>>(),
            output: Box::new(output),
        }
    }
    std::sync::LazyLock::new(|| {
        std::collections::HashMap::from(
        [
            (
                StillName::from("int-negate"),
                RustVariableItemKind::Fn,
                false,
                function([still_type_int], still_type_int),
                "Flip its sign",
            ),
            (
                StillName::from("int-absolute"),
                RustVariableItemKind::Fn,
                false,
                function([still_type_int], still_type_int),
                "If negative, negate",
            ),
            (
                StillName::from("int-add"),
                RustVariableItemKind::Fn,
                false,
                function([still_type_int,still_type_int], still_type_int),
                "Addition operation (`+`)",
            ),
            (
                StillName::from("int-mul"),
                RustVariableItemKind::Fn,
                false,
                function([still_type_int,still_type_int], still_type_int),
                "Multiplication operation (`*`)",
            ),
            (
                StillName::from("int-div"),
                RustVariableItemKind::Fn,
                false,
                function([still_type_int,still_type_int], still_type_int),
                "Integer division operation (`/`), discarding any remainder. Try not to divide by 0, as 0 will be returned which is not mathematically correct. This behaviour is consistent with gleam, pony, coq, lean",
            ),
            (
                StillName::from("int-order"),
                RustVariableItemKind::Fn,
                false,
                function([still_type_int,still_type_int], still_type_order),
                "Compare `int` values",
            ),
            (
                StillName::from("int-to-str"),
                RustVariableItemKind::Fn,
                true,
                function([still_type_int], still_type_str),
                "Convert `int` to `str`",
            ),
            (
                StillName::from("str-to-int"),
                RustVariableItemKind::Fn,
                false,
                function([still_type_str], still_type_opt(still_type_int)),
                "Parse a complete `str` into an `int`, returning :opt int:Absent otherwise",
            ),
            (
                StillName::from("dec-negate"),
                RustVariableItemKind::Fn,
                false,
                function([still_type_dec], still_type_dec),
                "Flip its sign",
            ),
            (
                StillName::from("dec-absolute"),
                RustVariableItemKind::Fn,
                false,
                function([still_type_dec], still_type_dec),
                "If negative, negate",
            ),
            (
                StillName::from("dec-add"),
                RustVariableItemKind::Fn,
                false,
                function([still_type_dec,still_type_dec], still_type_dec),
                "Addition operation (`+`)",
            ),
            (
                StillName::from("dec-mul"),
                RustVariableItemKind::Fn,
                false,
                function([still_type_dec,still_type_dec], still_type_dec),
                "Multiplication operation (`*`)",
            ),
            (
                StillName::from("dec-div"),
                RustVariableItemKind::Fn,
                false,
                function([still_type_dec,still_type_dec], still_type_dec),
                "Division operation (`/`). Try not to divide by 0.0, as 0.0 will be returned which is not mathematically correct. This behaviour is consistent with gleam, pony, coq, lean.",
            ),
            (
                StillName::from("dec-to-power-of"),
                RustVariableItemKind::Fn,
                false,
                function([still_type_dec,still_type_dec], still_type_dec),
                "Exponentiation operation (`^`)",
            ),
            (
                StillName::from("dec-truncate"),
                RustVariableItemKind::Fn,
                true,
                function([still_type_dec], still_type_int),
                "Its integer part, stripping away anything after the decimal point. Its like floor for positive inputs and ceiling for negative inputs",
            ),
            (
                StillName::from("dec-floor"),
                RustVariableItemKind::Fn,
                true,
                function([still_type_dec], still_type_int),
                "Its nearest smaller integer",
            ),
            (
                StillName::from("dec-ceiling"),
                RustVariableItemKind::Fn,
                true,
                function([still_type_dec], still_type_int),
                "Its nearest greater integer",
            ),
            (
                StillName::from("dec-round"),
                RustVariableItemKind::Fn,
                true,
                function([still_type_dec], still_type_int),
                "Its nearest integer. If the input ends in .5, round away from 0.0",
            ),
            (
                StillName::from("dec-order"),
                RustVariableItemKind::Fn,
                false,
                function([still_type_dec,still_type_dec], still_type_order),
                "Compare `dec` values",
            ),
            (
                StillName::from("dec-to-str"),
                RustVariableItemKind::Fn,
                true,
                function([still_type_dec], still_type_str),
                "Convert `dec` to `str`",
            ),
            (
                StillName::from("str-to-dec"),
                RustVariableItemKind::Fn,
                false,
                function([still_type_str], still_type_opt(still_type_dec)),
                "Parse a complete `str` into an `dec`, returning :opt dec:Absent otherwise",
            ),
            (
                StillName::from("chr-byte-count"),
                RustVariableItemKind::Fn,
                false,
                function([still_type_chr], still_type_int),
                "Encoded as UTF-8, how many bytes the `chr` spans, between 1 and 4",
            ),
            (
                StillName::from("chr-order"),
                RustVariableItemKind::Fn,
                false,
                function([still_type_chr,still_type_chr], still_type_order),
                "Compare `chr` values by their unicode code point",
            ),
            (
                StillName::from("chr-to-str"),
                RustVariableItemKind::Fn,
                true,
                function([still_type_chr], still_type_str),
                "Convert `chr` to `str`",
            ),
            (
                StillName::from("str-byte-count"),
                RustVariableItemKind::Fn,
                false,
                function([still_type_str], still_type_int),
                "Encoded as UTF-8, how many bytes the `str` spans",
            ),
            (
                StillName::from("str-chr-at-byte-index"),
                RustVariableItemKind::Fn,
                false,
                function(
                    [still_type_str, still_type_int],
                    still_type_opt(still_type_chr),
                ),
                "The `chr` at the nearest lower character boundary of a given UTF-8 index. If it lands out of bounds, results in :option Element:Absent",
            ),
            (
                StillName::from("str-slice-from-byte-index-with-byte-length"),
                RustVariableItemKind::Fn,
                false,
                function(
                    [still_type_str, still_type_int,still_type_int],
                    still_type_opt(still_type_str),
                ),
                "Create a sub-slice starting at the floor character boundary of a given UTF-8 index, spanning for a given count of UTF-8 bytes until before the nearest higher character boundary",
            ),
            (
                StillName::from("str-to-chrs"),
                RustVariableItemKind::Fn,
                true,
                function([still_type_str], still_type_vec(still_type_chr)),
                "Split the `str` into a `vec` of `chr`s",
            ),
            (
                StillName::from("chrs-to-str"),
                RustVariableItemKind::Fn,
                true,
                function([still_type_vec(still_type_chr)], still_type_str),
                "Concatenate a `vec` of `chr`s into one `str`",
            ),
            (
                StillName::from("str-order"),
                RustVariableItemKind::Fn,
                false,
                function([still_type_str,still_type_str], still_type_order),
                "Compare `str` values lexicographically (chr-wise comparison, then longer is greater). A detailed definition: https://doc.rust-lang.org/std/cmp/trait.Ord.html#lexicographical-comparison",
            ),
            (
                StillName::from("str-walk-chrs-from"),
                RustVariableItemKind::Fn,
                false,
                function(
                 [still_type_str,
                  function([variable("State"), still_type_chr], still_type_continue_or_exit(variable("State"), variable("Exit")))
                 ],
                 still_type_continue_or_exit(variable("State"), variable("Exit"))
                ),
                r"Loop through all of its `chr`s first to last, collecting state or exiting early
```still
str-find-spaces-in-first-line \:str:str >
    str-walk-chrs-from str
        0
        (\:int:space-count-so-far, :chr:chr >
            chr
            | '\n' > :continue-or-exit int int:Exit space-count-so-far
            | ' ' > 
                :continue-or-exit int int:
                Continue int-add space-count-so-far 1
            | :chr:_ >
                :continue-or-exit int int:Continue space-count-so-far
        )
    | :continue-or-exit int int:Continue :int:result > result
    | :continue-or-exit int int:Exit :int:result > result
```
As you're probably realizing, this is powerful but
both inconvenient and not very declarative (similar to a for each in loop in other languages).
I recommend creating helpers for common cases like mapping to an `opt` and keeping the `Present` ones.
",
            ),
            (
                StillName::from("strs-flatten"),
                RustVariableItemKind::Fn,
                true,
                function([still_type_vec(still_type_str)], still_type_str),
                "Concatenate all the string elements",
            ),
            (
                StillName::from("vec-repeat"),
                RustVariableItemKind::Fn,
                true,
                function([still_type_int, variable("A")], still_type_vec(variable("A"))),
                "Build a `vec` with a given length and a given element at each index",
            ),
            (
                StillName::from("vec-length"),
                RustVariableItemKind::Fn,
                false,
                function([still_type_vec(variable("A"))], still_type_int),
                "Its element count",
            ),
            (
                StillName::from("vec-element"),
                RustVariableItemKind::Fn,
                false,
                function(
                    [still_type_vec(variable("A")),still_type_int],
                    still_type_opt(variable("A")),
                ),
                "The element at a given index. If it lands out of bounds, results in :option Element:Absent",
            ),
            (
                StillName::from("vec-take"),
                RustVariableItemKind::Fn,
                false,
                function(
                    [still_type_vec(variable("A")), still_type_int],
                    still_type_vec(variable("A")),
                ),
                "Truncate to at most a given length",
            ),
            (
                StillName::from("vec-increase-capacity-by"),
                RustVariableItemKind::Fn,
                false,
                function(
                    [still_type_vec(variable("A")), still_type_int],
                    still_type_vec(variable("A")),
                ),
                "Reserve capacity for at least a given count of additional elements to be inserted in the given vec (reserving space is done automatically when inserting elements but when knowing more about the final size, we can avoid reallocations).",
            ),
            (
                StillName::from("vec-sort"),
                RustVariableItemKind::Fn,
                false,
                function(
                    [still_type_vec(variable("A")),
                     function([variable("A"),variable("A")], still_type_order)
                    ],
                    still_type_vec(variable("A")),
                ),
                "Reserve capacity for at least a given count of additional elements to be inserted in the given vec (reserving space is done automatically when inserting elements but when knowing more about the final size, we can avoid reallocations).",
            ),
            (
                StillName::from("vec-attach"),
                RustVariableItemKind::Fn,
                false,
                function([still_type_vec(variable("A")), still_type_vec(variable("A"))], still_type_vec(variable("A"))),
                "Glue the elements in a second `vec` after the first `vec`",
            ),
            (
                StillName::from("vec-flatten"),
                RustVariableItemKind::Fn,
                false,
                function([still_type_vec(still_type_vec(variable("A")))], still_type_vec(variable("A"))),
                "Concatenate all the elements nested inside the inner `vec`s",
            ),
            (
                StillName::from("vec-walk-from"),
                RustVariableItemKind::Fn,
                false,
                function(
                 [still_type_vec(variable("A")),
                  function([variable("State"),variable("A")], still_type_continue_or_exit(variable("State"), variable("Exit")))
                 ],
                 still_type_continue_or_exit(variable("State"), variable("Exit"))
                ),
                r"Loop through all of its elements first to last, collecting state or exiting early
```still
# if you aren't using any state in Continue, just use {}
vec-first-present \:vec (opt A):vec >
    vec-walk-from vec
        {}
        (\:opt A:element, {} >
            element
            | :opt A:Absent >
                :continue-or-exit {} A:Continue {}
            | :opt A:Present :A:found >
                :continue-or-exit {} A:Exit found
        )
    | :continue-or-exit {} A:Continue {} > :opt A:Absent
    | :continue-or-exit {} A:Exit :A:found > :opt A:Present found

# if you aren't calling Exit, you can use the same type as for the state
ints-sum \:vec int:vec >
    vec-walk-from vec
        0
        (\:int:sum-so-far, :int:element > :continue-or-exit int int:
            Continue int-add sum-so-far element
        )
    | :continue-or-exit int int:Continue :int:result > result
    | :continue-or-exit int int:Exit :int:result > result
```
As you're probably realizing, this is powerful but
both inconvenient and not very declarative (similar to a for each in loop in other languages).
I recommend creating helpers for common cases like mapping to an `opt` and keeping the `Present` ones.
",
            ),
        ]
        .map(|(name, kind, has_allocator_parameter, type_, documentation)| {
            // TODO inline
            (
                name,
                CompiledVariableDeclarationInfo {
                    name_range: None,
                    documentation: Some(Box::from(documentation)),
                    kind: kind,
                    type_: Some(type_),
                    has_allocator_parameter: has_allocator_parameter,
                },
            )
        }),
    )
    })
};
fn still_type_to_syntax_node(type_: &StillType) -> StillSyntaxNode<StillSyntaxType> {
    still_syntax_node_empty(match type_ {
        StillType::Variable(name) => StillSyntaxType::Variable(name.clone()),
        StillType::Function { inputs, output } => StillSyntaxType::Function {
            inputs: inputs.iter().map(still_type_to_syntax_node).collect(),
            arrow_key_symbol_range: None,
            output: Some(still_syntax_node_box(still_type_to_syntax_node(output))),
        },
        StillType::ChoiceConstruct { name, arguments } => StillSyntaxType::Construct {
            name: still_syntax_node_empty(name.clone()),
            arguments: arguments.iter().map(still_type_to_syntax_node).collect(),
        },
        StillType::Record(fields) => StillSyntaxType::Record(
            fields
                .iter()
                .map(|field| StillSyntaxTypeField {
                    name: still_syntax_node_empty(field.name.clone()),
                    value: Some(still_type_to_syntax_node(&field.value)),
                })
                .collect(),
        ),
    })
}
static core_choice_type_infos: std::sync::LazyLock<
    std::collections::HashMap<StillName, ChoiceTypeInfo>,
> = {
    std::sync::LazyLock::new(|| {
        std::collections::HashMap::from([
        (
            StillName::from(still_type_int_name),
            ChoiceTypeInfo {
                name_range: None,
                documentation: Some(Box::from(
                    r"A whole number (signed integer). Has the same size as a pointer on the target platform (so 64 bits on 64-bit platforms).
```still
vec-repeat 5 2
# = [ 2, 2, 2, 2, 2 ]
```
"
                )),
                parameters: vec![],
                variants: vec![],
                is_copy: true,
                has_owned_representation: true,
                has_lifetime_parameter: false,
                type_variants: vec![],
            },
        ),
        (
            StillName::from(still_type_dec_name),
            ChoiceTypeInfo {
                name_range: None,
                documentation: Some(Box::from(
                    r#"A signed floating point number. Has 64 bits of precision and behaves as specified by the "binary64" type defined in IEEE 754-2008.
```still
five
    # . or .0 is mandatory for dec,
    # otherwise the number is of type :int:
    5.0

dec-div five 2.0
# = 2.5
```
"#
                )),
                parameters: vec![],
                variants: vec![],
                is_copy: true,
                has_owned_representation: true,
                has_lifetime_parameter: false,
                type_variants: vec![],
            },
        ),
        (
            StillName::from(still_type_chr_name),
            ChoiceTypeInfo {
                name_range: None,
                documentation: Some(Box::from(
                    r#"A unicode scalar like `'a'` or `'👀'` or `\u{2665}` (hex code for ♥).
Keep in mind that a human-readable visual symbol can be composed of multiple such unicode scalars (forming a grapheme cluster), For example:
```still
str-to-chrs "🇺🇸"
# = [ '\u{1F1FA}', '\u{1F1F8}' ]
#     Indicator U  Indicator S
```
Read if interested: [swift's grapheme cluster docs](https://docs.swift.org/swift-book/documentation/the-swift-programming-language/stringsandcharacters/#Extended-Grapheme-Clusters)\
"#
                )),
                parameters: vec![],
                variants: vec![],
                is_copy: true,
                has_owned_representation: true,
                has_lifetime_parameter: false,
                type_variants: vec![],
            },
        ),
        (
            StillName::from(still_type_str_name),
            ChoiceTypeInfo {
                name_range: None,
                documentation: Some(Box::from(
                    r#"Immutable text (segment) like `"abc"` or `"\"hello 👀 \\\r\n world \u{2665}\""` (`\u{2665}` represents the hex code for ♥, `\"` represents ", `\\` represents \\, `\n` represents line break, `\r` represents carriage return).
Internally, a string is compactly represented as UTF-8 bytes and can be accessed as such.
```still
strs-flatten [ "My name is ", "Jenna", " and I'm ", int-to-str 60, " years old." ]
# = "My name is Jenna and I'm 60 years old."
```
Do not use plain `str` to build a big string.
"#
                )),
                parameters: vec![],
                variants: vec![],
                is_copy: true,
                has_owned_representation: true,
                has_lifetime_parameter: true,
                type_variants: vec![],
            },
        ),
        (
            StillName::from(still_type_order_name),
            ChoiceTypeInfo {
                name_range: None,
                documentation: Some(Box::from(
                    r#"The result of a comparison.
```still
int-cmp 1 2
# = :order:Less

dec-cmp 0.0 0.0
# = :order:Equal

chr-cmp 'b' 'a'
# = :order:Greater

# typically used with pattern matching
int-order x 5
| :order:Less >
    "must be >= 5"
| :order:_ >
int-order x 10
| :order:Greater >
    "must be <= 10"
| :order:_
    "valid"

# and is used for sorting
vec
```
If necessary you can create order functions for your specific types,
still does not have "traits"/"type classes" or similar, functions are always passed explicitly.
"#
                )),
                parameters: vec![still_syntax_node_empty(StillName::from("A"))],
                type_variants: vec![
                    StillChoiceTypeVariantInfo{
                        name:StillName::from("Absent"),
                        value: None
                    },
                    StillChoiceTypeVariantInfo{
                        name:StillName::from("Present"),
                        value: Some(StillChoiceTypeVariantValueInfo {
                            type_: StillType::Variable(StillName::from("A")),
                            constructs_recursive_type: false
                        })
                    }
                ],
                is_copy: true,
                has_owned_representation: true,
                has_lifetime_parameter: false,
                // should be able to be omitted
                variants: vec![
                    StillSyntaxChoiceTypeVariant {
                        or_key_symbol_range: lsp_types::Range::default(),
                        name: Some(still_syntax_node_empty(StillName::from("Absent"))),
                        value: None,
                    },
                    StillSyntaxChoiceTypeVariant {
                        or_key_symbol_range: lsp_types::Range::default(),
                        name: Some(still_syntax_node_empty(StillName::from("Present"))),
                        value: Some(still_syntax_node_empty(StillSyntaxType::Variable(
                            StillName::from("A"),
                        ))),
                    }
                ],
            },
        ),
        (
            StillName::from(still_type_opt_name),
            ChoiceTypeInfo {
                name_range: None,
                documentation: Some(Box::from(
                    r"Either you have some value or you have nothing."
                )),
                parameters: vec![still_syntax_node_empty(StillName::from("A"))],
                type_variants: vec![
                    StillChoiceTypeVariantInfo{
                        name:StillName::from("Absent"),
                        value: None
                    },
                    StillChoiceTypeVariantInfo{
                        name:StillName::from("Present"),
                        value: Some(StillChoiceTypeVariantValueInfo {
                            type_: StillType::Variable(StillName::from("A")),
                            constructs_recursive_type: false
                        })
                    }
                ],
                is_copy: true,
                has_owned_representation: true,
                has_lifetime_parameter: false,
                // should be able to be omitted
                variants: vec![
                    StillSyntaxChoiceTypeVariant {
                        or_key_symbol_range: lsp_types::Range::default(),
                        name: Some(still_syntax_node_empty(StillName::from("Absent"))),
                        value: None,
                    },
                    StillSyntaxChoiceTypeVariant {
                        or_key_symbol_range: lsp_types::Range::default(),
                        name: Some(still_syntax_node_empty(StillName::from("Present"))),
                        value: Some(still_syntax_node_empty(StillSyntaxType::Variable(
                            StillName::from("A"),
                        ))),
                    }
                ],
            },
        ),
        (
            StillName::from(still_type_continue_or_exit_name),
            ChoiceTypeInfo {
                name_range: None,
                documentation: Some(Box::from(
                    r"Either done with a final result or continuing with a partial result.
Typically used for operations that can shortcut.
```still
# If you aren't using any state in Continue, just use {}
vec-first-present \:vec (opt A):vec >
    vec-walk-from vec
        {}
        (\:opt A:element, {} >
            element
            | :opt A:Absent >
                :continue-or-exit {} A:Continue {}
            | :opt A:Present :A:found >
                :continue-or-exit {} A:Exit found
        )
    | :continue-or-exit {} A:Continue {} > :opt A:Absent
    | :continue-or-exit {} A:Exit :A:found > :opt A:Present found

loop-from \:State:state, :\State > continue-or-exit State Exit: step >
    step state
    | :continue-or-exit State Exit:Exit :Exit:exit > exit
    | :continue-or-exit State Exit:Continue :Continue:updated_state >
        loop_from updated_state step

numbers0-9
    loop_from { index 0, vec vec-increase-capacity-by (:vec int:[]) 10 }
        (\{ index i, vec vec } >
            int-order i 10
            | :order:Less >
                :continue-or-exit { index int, vec vec int } (vec int):
                Continue { index int-add i 1, vec vec-attach vec [ i ] }
            | :order:_ >
                :continue-or-exit { index int, vec vec int } (vec int):
                Exit vec
        )
```
"
                )),
                parameters: vec![still_syntax_node_empty(StillName::from("Continue")), still_syntax_node_empty(StillName::from("Exit"))],
                type_variants: vec![
                    StillChoiceTypeVariantInfo{
                        name:StillName::from("Continue"),
                        value: Some(StillChoiceTypeVariantValueInfo {
                            type_: StillType::Variable(StillName::from("Continue")),
                            constructs_recursive_type: false
                        })
                    },
                    StillChoiceTypeVariantInfo{
                        name:StillName::from("Exit"),
                        value: Some(StillChoiceTypeVariantValueInfo {
                            type_: StillType::Variable(StillName::from("Exit")),
                            constructs_recursive_type: false
                        })
                    }
                ],
                is_copy: true,
                has_owned_representation: true,
                has_lifetime_parameter: false,
                // should be able to be omitted
                variants: vec![
                    StillSyntaxChoiceTypeVariant {
                        or_key_symbol_range: lsp_types::Range::default(),
                        name: Some(still_syntax_node_empty(StillName::from("Absent"))),
                        value: None,
                    },
                    StillSyntaxChoiceTypeVariant {
                        or_key_symbol_range: lsp_types::Range::default(),
                        name: Some(still_syntax_node_empty(StillName::from("Present"))),
                        value: Some(still_syntax_node_empty(StillSyntaxType::Variable(
                            StillName::from("A"),
                        ))),
                    }
                ],
            },
        ),
        (
            StillName::from(still_type_vec_name),
            ChoiceTypeInfo {
                name_range: None,
                documentation: Some(Box::from(
                    "A growable array of elements. Arrays have constant time access and mutation and amortized constant time push.
```still
my-vec :vec int:
    [ 1, 2, 3 ]

vec-element 0 my-vec
# = :opt int:Present 1

vec-element 3 my-vec
# = :opt int:Absent
```
"
                )),
                parameters: vec![still_syntax_node_empty(StillName::from("A"))],
                variants: vec![],
                is_copy: false,
                has_owned_representation: true,
                has_lifetime_parameter: false,
                type_variants: vec![],
            },
        ),
        ])
    })
};

fn still_syntax_record_to_rust(used_still_record_fields: &[StillName]) -> [syn::Item; 3] {
    let rust_struct_name: String = still_field_names_to_rust_record_struct_name(
        used_still_record_fields.iter().map(StillName::as_str),
    );
    let rust_struct: syn::Item = syn::Item::Struct(syn::ItemStruct {
        attrs: vec![syn_attribute_derive(
            [
                "Copy",
                "Clone",
                "PartialEq",
                "Eq",
                "PartialOrd",
                "Ord",
                "Debug",
                "Hash",
            ]
            .into_iter(),
        )],
        vis: syn::Visibility::Public(syn::token::Pub(syn_span())),
        struct_token: syn::token::Struct(syn_span()),
        ident: syn_ident(&rust_struct_name),
        generics: syn::Generics {
            lt_token: Some(syn::token::Lt(syn_span())),
            params: used_still_record_fields
                .iter()
                .map(|field_name| {
                    syn::GenericParam::Type(syn::TypeParam {
                        attrs: vec![],
                        ident: syn_ident(&still_type_variable_to_rust(field_name)),
                        colon_token: None,
                        bounds: syn::punctuated::Punctuated::new(),
                        eq_token: None,
                        default: None,
                    })
                })
                .collect(),
            gt_token: Some(syn::token::Gt(syn_span())),
            where_clause: None,
        },
        fields: syn::Fields::Named(syn::FieldsNamed {
            brace_token: syn::token::Brace(syn_span()),
            named: used_still_record_fields
                .iter()
                .map(|field_name| syn::Field {
                    attrs: vec![],
                    vis: syn::Visibility::Public(syn::token::Pub(syn_span())),
                    mutability: syn::FieldMutability::None,
                    ident: Some(syn_ident(&still_name_to_lowercase_rust(field_name))),
                    colon_token: Some(syn::token::Colon(syn_span())),
                    ty: syn::Type::Path(syn::TypePath {
                        qself: None,
                        path: syn_path_reference([&still_type_variable_to_rust(field_name)]),
                    }),
                })
                .collect(),
        }),
        semi_token: None,
    });
    let impl_still_to_owned: syn::Item = syn::Item::Impl(syn::ItemImpl {
        attrs: vec![],
        defaultness: None,
        unsafety: None,
        impl_token: syn::token::Impl(syn_span()),
        generics: syn::Generics {
            lt_token: Some(syn::token::Lt(syn_span())),
            params: used_still_record_fields
                .iter()
                .map(|field_name| {
                    syn::GenericParam::Type(syn::TypeParam {
                        attrs: vec![],
                        ident: syn_ident(&still_type_variable_to_rust(field_name)),
                        colon_token: None,
                        bounds: std::iter::once(syn::TypeParamBound::Trait(syn::TraitBound {
                            paren_token: None,
                            modifier: syn::TraitBoundModifier::None,
                            lifetimes: None,
                            path: syn_path_reference(["StillIntoOwned"]),
                        }))
                        .collect(),
                        eq_token: None,
                        default: None,
                    })
                })
                .collect(),
            gt_token: Some(syn::token::Gt(syn_span())),
            where_clause: None,
        },
        trait_: Some((
            None,
            syn_path_reference(["StillIntoOwned"]),
            syn::token::For(syn_span()),
        )),
        self_ty: Box::new(syn::Type::Path(syn::TypePath {
            qself: None,
            path: syn_path_name_with_arguments(
                &rust_struct_name,
                used_still_record_fields.iter().map(|field_name| {
                    syn::GenericArgument::Type(syn::Type::Path(syn::TypePath {
                        qself: None,
                        path: syn_path_reference([&still_type_variable_to_rust(field_name)]),
                    }))
                }),
            ),
        })),
        brace_token: syn::token::Brace(syn_span()),
        items: vec![
            syn::ImplItem::Type(syn::ImplItemType {
                attrs: vec![],
                vis: syn::Visibility::Inherited,
                defaultness: None,
                type_token: syn::token::Type(syn_span()),
                ident: syn_ident("Owned"),
                generics: syn_generics_none(),
                eq_token: syn::token::Eq(syn_span()),
                ty: syn::Type::Path(syn::TypePath {
                    qself: None,
                    path: syn_path_name_with_arguments(
                        &rust_struct_name,
                        used_still_record_fields.iter().map(|field_name| {
                            syn::GenericArgument::Type(syn::Type::Path(syn::TypePath {
                                qself: None,
                                path: syn_path_reference([
                                    &still_type_variable_to_rust(field_name),
                                    "Owned",
                                ]),
                            }))
                        }),
                    ),
                }),
                semi_token: syn::token::Semi(syn_span()),
            }),
            syn::ImplItem::Fn(syn::ImplItemFn {
                attrs: vec![],
                vis: syn::Visibility::Inherited,
                defaultness: None,
                sig: syn::Signature {
                    constness: None,
                    asyncness: None,
                    unsafety: None,
                    abi: None,
                    fn_token: syn::token::Fn(syn_span()),
                    ident: syn_ident("into_owned"),
                    generics: syn_generics_none(),
                    paren_token: syn::token::Paren(syn_span()),
                    inputs: std::iter::once(syn::FnArg::Receiver(syn::Receiver {
                        attrs: vec![],
                        reference: None,
                        mutability: None,
                        self_token: syn::token::SelfValue(syn_span()),
                        colon_token: None,
                        ty: Box::new(syn::Type::Path(syn::TypePath {
                            qself: None,
                            path: syn_path_reference(["Self"]),
                        })),
                    }))
                    .collect(),
                    variadic: None,
                    output: syn::ReturnType::Type(
                        syn::token::RArrow(syn_span()),
                        Box::new(syn::Type::Path(syn::TypePath {
                            qself: None,
                            path: syn_path_reference(["Self", "Owned"]),
                        })),
                    ),
                },
                block: syn::Block {
                    brace_token: syn::token::Brace(syn_span()),
                    stmts: vec![syn::Stmt::Expr(
                        syn::Expr::Struct(syn::ExprStruct {
                            attrs: vec![],
                            qself: None,
                            path: syn_path_reference([&rust_struct_name]),
                            brace_token: syn::token::Brace(syn_span()),
                            fields: used_still_record_fields
                                .iter()
                                .map(|field_name| syn::FieldValue {
                                    attrs: vec![],
                                    member: syn::Member::Named(syn_ident(
                                        &still_name_to_lowercase_rust(field_name),
                                    )),
                                    colon_token: Some(syn::token::Colon(syn_span())),
                                    expr: syn::Expr::Call(syn::ExprCall {
                                        attrs: vec![],
                                        func: Box::new(syn_expr_reference([
                                            &still_type_variable_to_rust(field_name),
                                            "into_owned",
                                        ])),
                                        paren_token: syn::token::Paren(syn_span()),
                                        args: std::iter::once(syn::Expr::Field(syn::ExprField {
                                            attrs: vec![],
                                            base: Box::new(syn_expr_reference(["self"])),
                                            dot_token: syn::token::Dot(syn_span()),
                                            member: syn::Member::Named(syn_ident(
                                                &still_name_to_lowercase_rust(field_name),
                                            )),
                                        }))
                                        .collect(),
                                    }),
                                })
                                .collect(),
                            dot2_token: None,
                            rest: None,
                        }),
                        None,
                    )],
                },
            }),
        ],
    });
    let impl_owned_to_still: syn::Item = syn::Item::Impl(syn::ItemImpl {
        attrs: vec![],
        defaultness: None,
        unsafety: None,
        impl_token: syn::token::Impl(syn_span()),
        generics: syn::Generics {
            lt_token: Some(syn::token::Lt(syn_span())),
            params: used_still_record_fields
                .iter()
                .map(|field_name| {
                    syn::GenericParam::Type(syn::TypeParam {
                        attrs: vec![],
                        ident: syn_ident(&still_type_variable_to_rust(field_name)),
                        colon_token: None,
                        bounds: std::iter::once(syn::TypeParamBound::Trait(syn::TraitBound {
                            paren_token: None,
                            modifier: syn::TraitBoundModifier::None,
                            lifetimes: None,
                            path: syn_path_reference(["OwnedToStill"]),
                        }))
                        .collect(),
                        eq_token: None,
                        default: None,
                    })
                })
                .collect(),
            gt_token: Some(syn::token::Gt(syn_span())),
            where_clause: None,
        },
        trait_: Some((
            None,
            syn_path_reference(["OwnedToStill"]),
            syn::token::For(syn_span()),
        )),
        self_ty: Box::new(syn::Type::Path(syn::TypePath {
            qself: None,
            path: syn_path_name_with_arguments(
                &rust_struct_name,
                used_still_record_fields.iter().map(|field_name| {
                    syn::GenericArgument::Type(syn::Type::Path(syn::TypePath {
                        qself: None,
                        path: syn_path_reference([&still_type_variable_to_rust(field_name)]),
                    }))
                }),
            ),
        })),
        brace_token: syn::token::Brace(syn_span()),
        items: vec![
            syn::ImplItem::Type(syn::ImplItemType {
                attrs: vec![],
                vis: syn::Visibility::Inherited,
                defaultness: None,
                type_token: syn::token::Type(syn_span()),
                ident: syn_ident("Still"),
                generics: syn::Generics {
                    lt_token: Some(syn::token::Lt(syn_span())),
                    params: std::iter::once(syn::GenericParam::Lifetime(
                        syn_default_lifetime_param(),
                    ))
                    .collect(),
                    gt_token: Some(syn::token::Gt(syn_span())),
                    where_clause: Some(syn::WhereClause {
                        where_token: syn::token::Where(syn_span()),
                        predicates: used_still_record_fields
                            .iter()
                            .map(|field_name| {
                                syn::WherePredicate::Type(syn::PredicateType {
                                    lifetimes: None,
                                    bounded_ty: syn::Type::Path(syn::TypePath {
                                        qself: None,
                                        path: syn_path_reference([&still_type_variable_to_rust(
                                            field_name,
                                        )]),
                                    }),
                                    colon_token: syn::token::Colon(syn_span()),
                                    bounds: std::iter::once(syn::TypeParamBound::Lifetime(
                                        syn_default_lifetime(),
                                    ))
                                    .collect(),
                                })
                            })
                            .collect(),
                    }),
                },
                eq_token: syn::token::Eq(syn_span()),
                ty: syn::Type::Path(syn::TypePath {
                    qself: None,
                    path: syn_path_name_with_arguments(
                        &rust_struct_name,
                        used_still_record_fields.iter().map(|field_name| {
                            syn::GenericArgument::Type(syn::Type::Path(syn::TypePath {
                                qself: None,
                                path: syn::Path {
                                    leading_colon: None,
                                    segments: [
                                        syn_path_segment_ident(&still_type_variable_to_rust(
                                            field_name,
                                        )),
                                        syn::PathSegment {
                                            ident: syn_ident("Still"),
                                            arguments: syn::PathArguments::AngleBracketed(
                                                syn::AngleBracketedGenericArguments {
                                                    colon2_token: None,
                                                    lt_token: syn::token::Lt(syn_span()),
                                                    args: std::iter::once(
                                                        syn::GenericArgument::Lifetime(
                                                            syn_default_lifetime(),
                                                        ),
                                                    )
                                                    .collect(),
                                                    gt_token: syn::token::Gt(syn_span()),
                                                },
                                            ),
                                        },
                                    ]
                                    .into_iter()
                                    .collect(),
                                },
                            }))
                        }),
                    ),
                }),
                semi_token: syn::token::Semi(syn_span()),
            }),
            syn::ImplItem::Fn(syn::ImplItemFn {
                attrs: vec![],
                vis: syn::Visibility::Inherited,
                defaultness: None,
                sig: syn::Signature {
                    constness: None,
                    asyncness: None,
                    unsafety: None,
                    abi: None,
                    fn_token: syn::token::Fn(syn_span()),
                    ident: syn_ident("to_still"),
                    generics: syn::Generics {
                        lt_token: Some(syn::token::Lt(syn_span())),
                        params: std::iter::once(syn::GenericParam::Lifetime(
                            syn_default_lifetime_param(),
                        ))
                        .collect(),
                        gt_token: Some(syn::token::Gt(syn_span())),
                        where_clause: None,
                    },
                    paren_token: syn::token::Paren(syn_span()),
                    inputs: [
                        syn::FnArg::Receiver(syn::Receiver {
                            attrs: vec![],
                            reference: Some((
                                syn::token::And(syn_span()),
                                Some(syn_default_lifetime()),
                            )),
                            mutability: None,
                            self_token: syn::token::SelfValue(syn_span()),
                            colon_token: None,
                            ty: Box::new(syn::Type::Reference(syn::TypeReference {
                                and_token: syn::token::And(syn_span()),
                                lifetime: Some(syn_default_lifetime()),
                                mutability: None,
                                elem: Box::new(syn::Type::Path(syn::TypePath {
                                    qself: None,
                                    path: syn_path_reference(["Self"]),
                                })),
                            })),
                        }),
                        default_allocator_fn_arg(),
                    ]
                    .into_iter()
                    .collect(),
                    variadic: None,
                    output: syn::ReturnType::Type(
                        syn::token::RArrow(syn_span()),
                        Box::new(syn::Type::Path(syn::TypePath {
                            qself: None,
                            path: syn::Path {
                                leading_colon: None,
                                segments: [
                                    syn_path_segment_ident("Self"),
                                    syn::PathSegment {
                                        ident: syn_ident("Still"),
                                        arguments: syn::PathArguments::AngleBracketed(
                                            syn::AngleBracketedGenericArguments {
                                                colon2_token: None,
                                                lt_token: syn::token::Lt(syn_span()),
                                                args: std::iter::once(
                                                    syn::GenericArgument::Lifetime(
                                                        syn_default_lifetime(),
                                                    ),
                                                )
                                                .collect(),
                                                gt_token: syn::token::Gt(syn_span()),
                                            },
                                        ),
                                    },
                                ]
                                .into_iter()
                                .collect(),
                            },
                        })),
                    ),
                },
                block: syn::Block {
                    brace_token: syn::token::Brace(syn_span()),
                    stmts: vec![syn::Stmt::Expr(
                        syn::Expr::Struct(syn::ExprStruct {
                            attrs: vec![],
                            qself: None,
                            path: syn_path_reference([&rust_struct_name]),
                            brace_token: syn::token::Brace(syn_span()),
                            fields: used_still_record_fields
                                .iter()
                                .map(|field_name| syn::FieldValue {
                                    attrs: vec![],
                                    member: syn::Member::Named(syn_ident(
                                        &still_name_to_lowercase_rust(field_name),
                                    )),
                                    colon_token: Some(syn::token::Colon(syn_span())),
                                    expr: syn::Expr::Call(syn::ExprCall {
                                        attrs: vec![],
                                        func: Box::new(syn_expr_reference([
                                            &still_type_variable_to_rust(field_name),
                                            "to_still",
                                        ])),
                                        paren_token: syn::token::Paren(syn_span()),
                                        args: [
                                            syn::Expr::Reference(syn::ExprReference {
                                                attrs: vec![],
                                                and_token: syn::token::And(syn_span()),
                                                mutability: None,
                                                expr: Box::new(syn::Expr::Field(syn::ExprField {
                                                    attrs: vec![],
                                                    base: Box::new(syn_expr_reference(["self"])),
                                                    dot_token: syn::token::Dot(syn_span()),
                                                    member: syn::Member::Named(syn_ident(
                                                        &still_name_to_lowercase_rust(field_name),
                                                    )),
                                                })),
                                            }),
                                            syn_expr_reference([default_allocator_parameter_name]),
                                        ]
                                        .into_iter()
                                        .collect(),
                                    }),
                                })
                                .collect(),
                            dot2_token: None,
                            rest: None,
                        }),
                        None,
                    )],
                },
            }),
        ],
    });
    [rust_struct, impl_still_to_owned, impl_owned_to_still]
}
fn sorted_field_names<'a>(field_names: impl Iterator<Item = &'a StillName>) -> Vec<StillName> {
    let mut field_names_vec: Vec<StillName> = field_names.map(StillName::clone).collect();
    field_names_vec.sort_unstable();
    field_names_vec
}
fn still_syntax_type_declaration_connect_type_names_in_graph_from(
    type_graph: &mut strongly_connected_components::Graph,
    origin_type_declaration_graph_node: strongly_connected_components::Node,
    type_graph_node_by_name: &std::collections::HashMap<&str, strongly_connected_components::Node>,
    type_declaration_info: StillSyntaxTypeDeclarationInfo,
) {
    match type_declaration_info {
        StillSyntaxTypeDeclarationInfo::ChoiceType {
            documentation: _,
            name: _,
            parameters: _,
            variants,
        } => {
            for variant_value_node in variants.iter().filter_map(|variant| variant.value.as_ref()) {
                still_syntax_type_connect_type_names_in_graph_from(
                    type_graph,
                    origin_type_declaration_graph_node,
                    type_graph_node_by_name,
                    still_syntax_node_as_ref(variant_value_node),
                );
            }
        }
        StillSyntaxTypeDeclarationInfo::TypeAlias {
            documentation: _,
            name: _,
            parameters: _,
            type_: maybe_type,
        } => {
            if let Some(type_node) = maybe_type {
                still_syntax_type_connect_type_names_in_graph_from(
                    type_graph,
                    origin_type_declaration_graph_node,
                    type_graph_node_by_name,
                    still_syntax_node_as_ref(type_node),
                );
            }
        }
    }
}
fn still_syntax_type_connect_type_names_in_graph_from(
    type_graph: &mut strongly_connected_components::Graph,
    origin_type_declaration_graph_node: strongly_connected_components::Node,
    type_graph_node_by_name: &std::collections::HashMap<&str, strongly_connected_components::Node>,
    type_node: StillSyntaxNode<&StillSyntaxType>,
) {
    match type_node.value {
        StillSyntaxType::Variable(_) => {}
        StillSyntaxType::Parenthesized(maybe_in_parens) => {
            if let Some(in_parens_type_node) = maybe_in_parens {
                still_syntax_type_connect_type_names_in_graph_from(
                    type_graph,
                    origin_type_declaration_graph_node,
                    type_graph_node_by_name,
                    still_syntax_node_unbox(in_parens_type_node),
                );
            }
        }
        StillSyntaxType::WithComment {
            comment: _,
            type_: maybe_type_after_comment,
        } => {
            if let Some(after_comment_type_node) = maybe_type_after_comment {
                still_syntax_type_connect_type_names_in_graph_from(
                    type_graph,
                    origin_type_declaration_graph_node,
                    type_graph_node_by_name,
                    still_syntax_node_unbox(after_comment_type_node),
                );
            }
        }
        StillSyntaxType::Function {
            inputs,
            arrow_key_symbol_range: _,
            output: maybe_output,
        } => {
            for input_type_node in inputs {
                still_syntax_type_connect_type_names_in_graph_from(
                    type_graph,
                    origin_type_declaration_graph_node,
                    type_graph_node_by_name,
                    still_syntax_node_as_ref(input_type_node),
                );
            }
            if let Some(output_type_node) = maybe_output {
                still_syntax_type_connect_type_names_in_graph_from(
                    type_graph,
                    origin_type_declaration_graph_node,
                    type_graph_node_by_name,
                    still_syntax_node_unbox(output_type_node),
                );
            }
        }
        StillSyntaxType::Construct {
            name: name_node,
            arguments,
        } => {
            if let Some(constructed_type_name_graph_node) = type_graph_node_by_name
                .get(&name_node.value as &str)
                .copied()
            {
                type_graph.new_edge(
                    origin_type_declaration_graph_node,
                    constructed_type_name_graph_node,
                );
            }
            for argument_type_node in arguments {
                still_syntax_type_connect_type_names_in_graph_from(
                    type_graph,
                    origin_type_declaration_graph_node,
                    type_graph_node_by_name,
                    still_syntax_node_as_ref(argument_type_node),
                );
            }
        }
        StillSyntaxType::Record(fields) => {
            for field in fields {
                if let Some(output_type_node) = &field.value {
                    still_syntax_type_connect_type_names_in_graph_from(
                        type_graph,
                        origin_type_declaration_graph_node,
                        type_graph_node_by_name,
                        still_syntax_node_as_ref(output_type_node),
                    );
                }
            }
        }
    }
}
fn still_syntax_expression_connect_variables_in_graph_from(
    variable_graph: &mut strongly_connected_components::Graph,
    origin_variable_declaration_graph_node: strongly_connected_components::Node,
    variable_graph_node_by_name: &std::collections::HashMap<
        &str,
        strongly_connected_components::Node,
    >,
    expression_node: StillSyntaxNode<&StillSyntaxExpression>,
) {
    match expression_node.value {
        StillSyntaxExpression::Char(_) => {}
        StillSyntaxExpression::Dec(_) => {}
        StillSyntaxExpression::Int { .. } => {}
        StillSyntaxExpression::String { .. } => {}
        StillSyntaxExpression::VariableOrCall {
            variable: variable_node,
            arguments,
        } => {
            if let Some(variable_graph_node) = variable_graph_node_by_name
                .get(&variable_node.value as &str)
                .copied()
            {
                variable_graph
                    .new_edge(origin_variable_declaration_graph_node, variable_graph_node);
            }
            for argument_node in arguments {
                still_syntax_expression_connect_variables_in_graph_from(
                    variable_graph,
                    origin_variable_declaration_graph_node,
                    variable_graph_node_by_name,
                    still_syntax_node_as_ref(argument_node),
                );
            }
        }
        StillSyntaxExpression::Match {
            matched: matched_node,
            cases,
        } => {
            still_syntax_expression_connect_variables_in_graph_from(
                variable_graph,
                origin_variable_declaration_graph_node,
                variable_graph_node_by_name,
                still_syntax_node_unbox(matched_node),
            );
            for case in cases {
                if let Some(field_value_node) = &case.result {
                    still_syntax_expression_connect_variables_in_graph_from(
                        variable_graph,
                        origin_variable_declaration_graph_node,
                        variable_graph_node_by_name,
                        still_syntax_node_as_ref(field_value_node),
                    );
                }
            }
        }
        StillSyntaxExpression::Lambda {
            parameters: _,
            arrow_key_symbol_range: _,
            result: maybe_result,
        } => {
            if let Some(result_node) = maybe_result {
                still_syntax_expression_connect_variables_in_graph_from(
                    variable_graph,
                    origin_variable_declaration_graph_node,
                    variable_graph_node_by_name,
                    still_syntax_node_unbox(result_node),
                );
            }
        }
        StillSyntaxExpression::Let {
            declaration: maybe_declaration,
            result: maybe_result,
        } => {
            if let Some(declaration_node) = maybe_declaration
                && let Some(variable_result_expression_node) = &declaration_node.value.result
            {
                still_syntax_expression_connect_variables_in_graph_from(
                    variable_graph,
                    origin_variable_declaration_graph_node,
                    variable_graph_node_by_name,
                    still_syntax_node_unbox(variable_result_expression_node),
                );
            }
            if let Some(result_node) = maybe_result {
                still_syntax_expression_connect_variables_in_graph_from(
                    variable_graph,
                    origin_variable_declaration_graph_node,
                    variable_graph_node_by_name,
                    still_syntax_node_unbox(result_node),
                );
            }
        }
        StillSyntaxExpression::Vec(elements) => {
            for element_node in elements {
                still_syntax_expression_connect_variables_in_graph_from(
                    variable_graph,
                    origin_variable_declaration_graph_node,
                    variable_graph_node_by_name,
                    still_syntax_node_as_ref(element_node),
                );
            }
        }
        StillSyntaxExpression::Parenthesized(maybe_in_parens) => {
            if let Some(in_parens_node) = maybe_in_parens {
                still_syntax_expression_connect_variables_in_graph_from(
                    variable_graph,
                    origin_variable_declaration_graph_node,
                    variable_graph_node_by_name,
                    still_syntax_node_unbox(in_parens_node),
                );
            }
        }
        StillSyntaxExpression::WithComment {
            comment: _,
            expression: maybe_expression_after_comment,
        } => {
            if let Some(expression_node_after_comment) = maybe_expression_after_comment {
                still_syntax_expression_connect_variables_in_graph_from(
                    variable_graph,
                    origin_variable_declaration_graph_node,
                    variable_graph_node_by_name,
                    still_syntax_node_unbox(expression_node_after_comment),
                );
            }
        }
        StillSyntaxExpression::Typed {
            type_: _,
            expression: expression_in_typed,
        } => {
            if let Some(expression_node_in_typed) = expression_in_typed {
                match &expression_node_in_typed.value {
                    StillSyntaxExpressionUntyped::Variant {
                        name: _,
                        value: maybe_variant_value,
                    } => {
                        if let Some(variant_value_node) = maybe_variant_value {
                            still_syntax_expression_connect_variables_in_graph_from(
                                variable_graph,
                                origin_variable_declaration_graph_node,
                                variable_graph_node_by_name,
                                still_syntax_node_unbox(variant_value_node),
                            );
                        }
                    }
                    StillSyntaxExpressionUntyped::Other(other_expression_in_typed) => {
                        still_syntax_expression_connect_variables_in_graph_from(
                            variable_graph,
                            origin_variable_declaration_graph_node,
                            variable_graph_node_by_name,
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
                if let Some(field_value_node) = &field.value {
                    still_syntax_expression_connect_variables_in_graph_from(
                        variable_graph,
                        origin_variable_declaration_graph_node,
                        variable_graph_node_by_name,
                        still_syntax_node_as_ref(field_value_node),
                    );
                }
            }
        }
        StillSyntaxExpression::RecordAccess {
            record: record_node,
            field: _,
        } => {
            still_syntax_expression_connect_variables_in_graph_from(
                variable_graph,
                origin_variable_declaration_graph_node,
                variable_graph_node_by_name,
                still_syntax_node_unbox(record_node),
            );
        }
        StillSyntaxExpression::RecordUpdate {
            record: maybe_updated_record,
            spread_key_symbol_range: _,
            fields,
        } => {
            if let Some(updated_record_node) = maybe_updated_record {
                still_syntax_expression_connect_variables_in_graph_from(
                    variable_graph,
                    origin_variable_declaration_graph_node,
                    variable_graph_node_by_name,
                    still_syntax_node_unbox(updated_record_node),
                );
            }
            for field in fields {
                if let Some(field_value_node) = &field.value {
                    still_syntax_expression_connect_variables_in_graph_from(
                        variable_graph,
                        origin_variable_declaration_graph_node,
                        variable_graph_node_by_name,
                        still_syntax_node_as_ref(field_value_node),
                    );
                }
            }
        }
    }
}
struct CompiledTypeAlias {
    rust: syn::Item,
    is_copy: bool,
    has_owned_representation: bool,
    has_lifetime_parameter: bool,
    type_: StillType,
}
fn type_alias_declaration_to_rust(
    errors: &mut Vec<StillErrorNode>,
    records_used: &mut std::collections::HashSet<Vec<StillName>>,
    type_aliases: &std::collections::HashMap<StillName, TypeAliasInfo>,
    choice_types: &std::collections::HashMap<StillName, ChoiceTypeInfo>,
    maybe_documentation: Option<&str>,
    name_node: StillSyntaxNode<&StillName>,
    parameters: &[StillSyntaxNode<StillName>],
    maybe_type: Option<StillSyntaxNode<&StillSyntaxType>>,
) -> Option<CompiledTypeAlias> {
    let rust_name: String = still_name_to_uppercase_rust(name_node.value);
    let Some(type_node) = maybe_type else {
        errors.push(StillErrorNode {
            range: name_node.range,
            message: Box::from("type alias declaration is missing a type the given name is equal to after type alias ..type-name.. = here"),
        });
        return None;
    };
    let Some(type_) = still_syntax_type_to_type(errors, type_aliases, choice_types, type_node)
    else {
        return None;
    };
    let type_rust: syn::Type = still_type_to_rust(
        type_aliases,
        choice_types,
        syn_default_lifetime_name,
        FnRepresentation::RefDyn,
        &type_,
    );
    let has_lifetime_parameter: bool = still_type_uses_lifetime(type_aliases, choice_types, &type_);
    let mut actually_used_type_variables: std::collections::HashSet<StillName> =
        std::collections::HashSet::new();
    still_type_variables_and_records_into(&mut actually_used_type_variables, records_used, &type_);
    let mut rust_parameters: syn::punctuated::Punctuated<syn::GenericParam, syn::token::Comma> =
        syn::punctuated::Punctuated::new();
    if has_lifetime_parameter {
        rust_parameters.push(syn::GenericParam::Lifetime(syn_default_lifetime_param()));
    }
    if let Err(()) = still_parameters_to_rust_into_error_if_different_to_actual_type_parameters(
        errors,
        &mut rust_parameters,
        name_node.range,
        parameters,
        actually_used_type_variables,
    ) {
        return None;
    }
    Some(CompiledTypeAlias {
        rust: syn::Item::Type(syn::ItemType {
            attrs: maybe_documentation
                .map(syn_attribute_doc)
                .into_iter()
                .collect::<Vec<_>>(),
            vis: syn::Visibility::Public(syn::token::Pub(syn_span())),
            type_token: syn::token::Type(syn_span()),
            ident: syn_ident(&rust_name),
            generics: syn::Generics {
                lt_token: Some(syn::token::Lt(syn_span())),
                params: rust_parameters,
                gt_token: Some(syn::token::Gt(syn_span())),
                where_clause: None,
            },
            eq_token: syn::token::Eq(syn_span()),
            ty: Box::new(type_rust),
            semi_token: syn::token::Semi(syn_span()),
        }),
        has_lifetime_parameter: has_lifetime_parameter,
        is_copy: still_type_is_copy(true, type_aliases, choice_types, &type_),
        has_owned_representation: still_type_has_owned_representation(
            true,
            type_aliases,
            choice_types,
            &type_,
        ),
        type_: type_,
    })
}
/// returns false if
fn still_parameters_to_rust_into_error_if_different_to_actual_type_parameters(
    errors: &mut Vec<StillErrorNode>,
    rust_parameters: &mut syn::punctuated::Punctuated<syn::GenericParam, syn::token::Comma>,
    name_range: lsp_types::Range,
    parameters: &[StillSyntaxNode<StillName>],
    mut actually_used_type_variables: std::collections::HashSet<StillName>,
) -> Result<(), ()> {
    let mut bad_parameters: bool = false;
    for parameter_node in parameters {
        if !actually_used_type_variables.remove(parameter_node.value.as_str()) {
            bad_parameters = true;
            errors.push(StillErrorNode {
                range: parameter_node.range,
                message: Box::from("this type variable is not used. Remove it or use it"),
            });
        }
        rust_parameters.push(syn::GenericParam::Type(syn::TypeParam::from(syn_ident(
            &still_type_variable_to_rust(&parameter_node.value),
        ))));
    }
    if !actually_used_type_variables.is_empty() {
        bad_parameters = true;
        errors.push(StillErrorNode {
            range: name_range,
            message: format!(
                "some type variables are used but not declared, namely {}. Add {}",
                actually_used_type_variables
                    .iter()
                    .map(StillName::as_str)
                    .collect::<Vec<&str>>()
                    .join(", "),
                if actually_used_type_variables.len() >= 2 {
                    "them"
                } else {
                    "it"
                }
            )
            .into_boxed_str(),
        });
    }
    if bad_parameters { Err(()) } else { Ok(()) }
}

struct CompiledRustChoiceTypeInfo {
    is_copy: bool,
    has_owned_representation: bool,
    has_lifetime_parameter: bool,
    variants: Vec<StillChoiceTypeVariantInfo>,
}
#[derive(Clone)]
struct StillChoiceTypeVariantInfo {
    name: StillName,
    value: Option<StillChoiceTypeVariantValueInfo>,
}
#[derive(Clone)]
struct StillChoiceTypeVariantValueInfo {
    type_: StillType,
    constructs_recursive_type: bool,
}
fn choice_type_declaration_to_rust_into<'a>(
    rust_items: &mut Vec<syn::Item>,
    errors: &mut Vec<StillErrorNode>,
    records_used: &mut std::collections::HashSet<Vec<StillName>>,
    type_aliases: &std::collections::HashMap<StillName, TypeAliasInfo>,
    choice_types: &std::collections::HashMap<StillName, ChoiceTypeInfo>,
    scc_type_declaration_names: &std::collections::HashSet<&str>,
    maybe_documentation: Option<&str>,
    name_node: StillSyntaxNode<&StillName>,
    parameters: &'a [StillSyntaxNode<StillName>],
    variants: &'a [StillSyntaxChoiceTypeVariant],
) -> Option<CompiledRustChoiceTypeInfo> {
    let mut rust_variants: syn::punctuated::Punctuated<syn::Variant, syn::token::Comma> =
        syn::punctuated::Punctuated::new();
    let mut type_variants: Vec<StillChoiceTypeVariantInfo> =
        Vec::with_capacity(rust_variants.len());
    let mut has_lifetime_parameter: bool = false;
    let mut is_copy: bool = true;
    let mut has_owned_representation: bool = true;
    let mut actually_used_type_variables: std::collections::HashSet<StillName> =
        std::collections::HashSet::with_capacity(parameters.len());
    for variant in variants {
        match &variant.name {
            None => {
                // no point in generating a variant since it's never referenced
                errors.push(StillErrorNode {
                    range: variant.or_key_symbol_range,
                    message: Box::from("missing variant name"),
                });
            }
            Some(variant_name) => {
                match &variant.value {
                    None => {
                        type_variants.push(StillChoiceTypeVariantInfo {
                            name: variant_name.value.clone(),
                            value: None,
                        });
                        rust_variants.push(syn::Variant {
                            attrs: vec![],
                            ident: syn_ident(&still_name_to_uppercase_rust(&variant_name.value)),
                            fields: syn::Fields::Unit,
                            discriminant: None,
                        });
                    }
                    Some(variant_value_node) => {
                        let Some(value_type) = still_syntax_type_to_type(
                            errors,
                            type_aliases,
                            choice_types,
                            still_syntax_node_as_ref(variant_value_node),
                        ) else {
                            // TODO instead, remember it failed but collect remaining errors,
                            //   pretend value type is None
                            return None;
                        };
                        let variant_value_constructs_recursive_type: bool =
                            still_type_constructs_recursive_type_in(
                                scc_type_declaration_names,
                                &value_type,
                            );
                        has_lifetime_parameter = if variant_value_constructs_recursive_type {
                            true
                        } else {
                            has_lifetime_parameter
                                || still_type_uses_lifetime(type_aliases, choice_types, &value_type)
                        };
                        if !variant_value_constructs_recursive_type {
                            is_copy = is_copy
                                && still_type_is_copy(
                                    true,
                                    type_aliases,
                                    choice_types,
                                    &value_type,
                                );
                        }
                        has_owned_representation = has_owned_representation
                            && still_type_has_owned_representation(
                                true,
                                type_aliases,
                                choice_types,
                                &value_type,
                            );
                        still_type_variables_and_records_into(
                            &mut actually_used_type_variables,
                            records_used,
                            &value_type,
                        );
                        let rust_variant_value: syn::Type = still_type_to_rust(
                            type_aliases,
                            choice_types,
                            syn_default_lifetime_name,
                            FnRepresentation::RefDyn,
                            &value_type,
                        );
                        type_variants.push(StillChoiceTypeVariantInfo {
                            name: variant_name.value.clone(),
                            value: Some(StillChoiceTypeVariantValueInfo {
                                type_: value_type,
                                constructs_recursive_type: variant_value_constructs_recursive_type,
                            }),
                        });
                        rust_variants.push(syn::Variant {
                            attrs: vec![],
                            ident: syn_ident(&still_name_to_uppercase_rust(&variant_name.value)),
                            fields: syn::Fields::Unnamed(syn::FieldsUnnamed {
                                paren_token: syn::token::Paren(syn_span()),
                                unnamed: std::iter::once(syn::Field {
                                    attrs: vec![],
                                    vis: syn::Visibility::Inherited,
                                    mutability: syn::FieldMutability::None,
                                    ident: None,
                                    colon_token: None,
                                    ty: if variant_value_constructs_recursive_type {
                                        syn::Type::Reference(syn::TypeReference {
                                            and_token: syn::token::And(syn_span()),
                                            lifetime: Some(syn_default_lifetime()),
                                            mutability: None,
                                            elem: Box::new(rust_variant_value),
                                        })
                                    } else {
                                        rust_variant_value
                                    },
                                })
                                .collect(),
                            }),
                            discriminant: None,
                        });
                    }
                }
            }
        }
    }
    let mut rust_parameters: syn::punctuated::Punctuated<syn::GenericParam, syn::token::Comma> =
        syn::punctuated::Punctuated::new();
    if has_lifetime_parameter {
        rust_parameters.push(syn::GenericParam::Lifetime(syn_default_lifetime_param()));
    }
    if let Err(()) = still_parameters_to_rust_into_error_if_different_to_actual_type_parameters(
        errors,
        &mut rust_parameters,
        name_node.range,
        parameters,
        actually_used_type_variables,
    ) {
        return None;
    }
    let rust_enum_name: String = still_name_to_uppercase_rust(name_node.value);
    rust_items.push(syn::Item::Enum(syn::ItemEnum {
        attrs: maybe_documentation
            .map(syn_attribute_doc)
            .into_iter()
            .chain(std::iter::once(syn_attribute_derive(
                std::iter::once("Clone").chain(if is_copy { Some("Copy") } else { None }),
            )))
            .collect::<Vec<_>>(),
        vis: syn::Visibility::Public(syn::token::Pub(syn_span())),
        enum_token: syn::token::Enum(syn_span()),
        ident: syn_ident(&rust_enum_name),
        generics: syn::Generics {
            lt_token: Some(syn::token::Lt(syn_span())),
            params: rust_parameters,
            gt_token: Some(syn::token::Gt(syn_span())),
            where_clause: None,
        },
        brace_token: syn::token::Brace(syn_span()),
        variants: rust_variants,
    }));
    if has_owned_representation {
        let owned_rust_enum_name: String = rust_enum_name.clone() + "øOwned";
        let owned_rust_variants: syn::punctuated::Punctuated<syn::Variant, syn::token::Comma> =
            type_variants
                .iter()
                .map(|variant| syn::Variant {
                    attrs: vec![],
                    ident: syn_ident(&still_name_to_uppercase_rust(&variant.name)),
                    fields: match &variant.value {
                        None => syn::Fields::Unit,
                        Some(variant_value) => {
                            let owned_rust_variant_value: syn::Type =
                                syn::Type::Path(syn::TypePath {
                                    qself: Some(syn::QSelf {
                                        lt_token: syn::token::Lt(syn_span()),
                                        ty: Box::new(still_type_to_rust(
                                            type_aliases,
                                            choice_types,
                                            syn_static_lifetime_name,
                                            FnRepresentation::RefDyn,
                                            &variant_value.type_,
                                        )),
                                        position: 1,
                                        as_token: Some(syn::token::As(syn_span())),
                                        gt_token: syn::token::Gt(syn_span()),
                                    }),
                                    path: syn_path_reference(["StillIntoOwned", "Owned"]),
                                });
                            let value_rust_type: syn::Type =
                                if variant_value.constructs_recursive_type {
                                    syn::Type::Path(syn::TypePath {
                                        qself: None,
                                        path: syn::Path {
                                            leading_colon: None,
                                            segments: [
                                                syn_path_segment_ident("std"),
                                                syn_path_segment_ident("boxed"),
                                                syn::PathSegment {
                                                    ident: syn_ident("Box"),
                                                    arguments: syn::PathArguments::AngleBracketed(
                                                        syn::AngleBracketedGenericArguments {
                                                            colon2_token: None,
                                                            lt_token: syn::token::Lt(syn_span()),
                                                            args: std::iter::once(
                                                                syn::GenericArgument::Type(
                                                                    owned_rust_variant_value,
                                                                ),
                                                            )
                                                            .collect(),
                                                            gt_token: syn::token::Gt(syn_span()),
                                                        },
                                                    ),
                                                },
                                            ]
                                            .into_iter()
                                            .collect(),
                                        },
                                    })
                                } else {
                                    owned_rust_variant_value
                                };
                            syn::Fields::Unnamed(syn::FieldsUnnamed {
                                paren_token: syn::token::Paren(syn_span()),
                                unnamed: std::iter::once(syn::Field {
                                    attrs: vec![],
                                    vis: syn::Visibility::Inherited,
                                    mutability: syn::FieldMutability::None,
                                    ident: None,
                                    colon_token: None,
                                    ty: value_rust_type,
                                })
                                .collect::<syn::punctuated::Punctuated<_, _>>(),
                            })
                        }
                    },
                    discriminant: None,
                })
                .collect();
        let rust_owned_enum: syn::Item = syn::Item::Enum(syn::ItemEnum {
            attrs: maybe_documentation
                .map(syn_attribute_doc)
                .into_iter()
                .chain(std::iter::once(syn_attribute_derive(std::iter::once(
                    "Clone",
                ))))
                .collect::<Vec<_>>(),
            vis: syn::Visibility::Public(syn::token::Pub(syn_span())),
            enum_token: syn::token::Enum(syn_span()),
            ident: syn_ident(&owned_rust_enum_name),
            generics: syn::Generics {
                lt_token: Some(syn::token::Lt(syn_span())),
                params: parameters
                    .iter()
                    .map(|parameter_node| {
                        syn::GenericParam::Type(syn::TypeParam {
                            attrs: vec![],
                            ident: syn_ident(&still_type_variable_to_rust(&parameter_node.value)),
                            colon_token: Some(syn::token::Colon(syn_span())),
                            bounds: [
                                syn::TypeParamBound::Trait(syn::TraitBound {
                                    paren_token: None,
                                    modifier: syn::TraitBoundModifier::None,
                                    lifetimes: None,
                                    path: syn::Path::from(syn_ident("StillIntoOwned")),
                                }),
                                // because still vec requires Clone for into_owned
                                syn::TypeParamBound::Trait(syn::TraitBound {
                                    paren_token: None,
                                    modifier: syn::TraitBoundModifier::None,
                                    lifetimes: None,
                                    path: syn::Path::from(syn_ident("Clone")),
                                }),
                            ]
                            .into_iter()
                            .collect(),
                            eq_token: None,
                            default: None,
                        })
                    })
                    .collect(),
                gt_token: Some(syn::token::Gt(syn_span())),
                where_clause: None,
            },
            brace_token: syn::token::Brace(syn_span()),
            variants: owned_rust_variants,
        });
        const variable_value_variable_name: &str = "value";
        let impl_still_to_owned: syn::Item = syn::Item::Impl(syn::ItemImpl {
            attrs: vec![],
            defaultness: None,
            unsafety: None,
            impl_token: syn::token::Impl(syn_span()),
            generics: syn::Generics {
                lt_token: Some(syn::token::Lt(syn_span())),
                params: (if has_lifetime_parameter {
                    Some(syn::GenericParam::Lifetime(syn_default_lifetime_param()))
                } else {
                    None
                })
                .into_iter()
                .chain(parameters.iter().map(|parameter_node| {
                    syn::GenericParam::Type(syn::TypeParam {
                        attrs: vec![],
                        ident: syn_ident(&still_type_variable_to_rust(&parameter_node.value)),
                        colon_token: Some(syn::token::Colon(syn_span())),
                        bounds: [
                            syn::TypeParamBound::Trait(syn::TraitBound {
                                paren_token: None,
                                modifier: syn::TraitBoundModifier::None,
                                lifetimes: None,
                                path: syn::Path::from(syn_ident("StillIntoOwned")),
                            }),
                            // because still vec requires Clone for into_owned
                            syn::TypeParamBound::Trait(syn::TraitBound {
                                paren_token: None,
                                modifier: syn::TraitBoundModifier::None,
                                lifetimes: None,
                                path: syn::Path::from(syn_ident("Clone")),
                            }),
                        ]
                        .into_iter()
                        .collect(),
                        eq_token: None,
                        default: None,
                    })
                }))
                .collect(),
                gt_token: Some(syn::token::Gt(syn_span())),
                where_clause: None,
            },
            trait_: Some((
                None,
                syn_path_reference(["StillIntoOwned"]),
                syn::token::For(syn_span()),
            )),
            self_ty: Box::new(syn::Type::Path(syn::TypePath {
                qself: None,
                path: syn_path_name_with_arguments(
                    &rust_enum_name,
                    (if has_lifetime_parameter {
                        Some(syn::GenericArgument::Lifetime(syn_default_lifetime()))
                    } else {
                        None
                    })
                    .into_iter()
                    .chain(parameters.iter().map(|parameter_node| {
                        syn::GenericArgument::Type(syn_type_variable(&still_type_variable_to_rust(
                            &parameter_node.value,
                        )))
                    })),
                ),
            })),
            brace_token: syn::token::Brace(syn_span()),
            items: vec![
                syn::ImplItem::Type(syn::ImplItemType {
                    attrs: vec![],
                    vis: syn::Visibility::Inherited,
                    defaultness: None,
                    type_token: syn::token::Type(syn_span()),
                    ident: syn_ident("Owned"),
                    generics: syn_generics_none(),
                    eq_token: syn::token::Eq(syn_span()),
                    ty: syn::Type::Path(syn::TypePath {
                        qself: None,
                        path: syn_path_name_with_arguments(
                            &owned_rust_enum_name,
                            parameters.iter().map(|parameter_node| {
                                syn::GenericArgument::Type(syn::Type::Path(syn::TypePath {
                                    qself: None,
                                    path: syn_path_reference([&still_type_variable_to_rust(
                                        &parameter_node.value,
                                    )]),
                                }))
                            }),
                        ),
                    }),
                    semi_token: syn::token::Semi(syn_span()),
                }),
                syn::ImplItem::Fn(syn::ImplItemFn {
                    attrs: vec![],
                    vis: syn::Visibility::Inherited,
                    defaultness: None,
                    sig: syn::Signature {
                        constness: None,
                        asyncness: None,
                        unsafety: None,
                        abi: None,
                        fn_token: syn::token::Fn(syn_span()),
                        ident: syn_ident("into_owned"),
                        generics: syn_generics_none(),
                        paren_token: syn::token::Paren(syn_span()),
                        inputs: std::iter::once(syn::FnArg::Receiver(syn::Receiver {
                            attrs: vec![],
                            reference: None,
                            mutability: None,
                            self_token: syn::token::SelfValue(syn_span()),
                            colon_token: None,
                            ty: Box::new(syn::Type::Path(syn::TypePath {
                                qself: None,
                                path: syn_path_reference(["Self"]),
                            })),
                        }))
                        .collect(),
                        variadic: None,
                        output: syn::ReturnType::Type(
                            syn::token::RArrow(syn_span()),
                            Box::new(syn::Type::Path(syn::TypePath {
                                qself: None,
                                path: syn_path_reference(["Self", "Owned"]),
                            })),
                        ),
                    },
                    block: syn::Block {
                        brace_token: syn::token::Brace(syn_span()),
                        stmts: vec![syn::Stmt::Expr(
                            syn::Expr::Match(syn::ExprMatch {
                                attrs: vec![],
                                match_token: syn::token::Match(syn_span()),
                                brace_token: syn::token::Brace(syn_span()),
                                expr: Box::new(syn_expr_reference(["self"])),
                                arms: variants
                                    .iter()
                                    .filter_map(|variant| {
                                        variant.name.as_ref().map(|n| {
                                            (
                                                still_syntax_node_as_ref_map(n, StillName::as_str),
                                                variant
                                                    .value
                                                    .as_ref()
                                                    .map(still_syntax_node_as_ref),
                                            )
                                        })
                                    })
                                    .map(|(variant_name, maybe_variant_value)| {
                                        let rust_variant_name: String =
                                            still_name_to_uppercase_rust(variant_name.value);
                                        let rust_pat_variant_path = syn_path_reference([
                                            &rust_enum_name,
                                            &rust_variant_name,
                                        ]);
                                        let rust_result_variant_constructor = syn_expr_reference([
                                            &owned_rust_enum_name,
                                            &rust_variant_name,
                                        ]);
                                        syn::Arm {
                                            attrs: vec![],
                                            guard: None,
                                            pat: match maybe_variant_value {
                                                None => syn::Pat::Path(syn::PatPath {
                                                    attrs: vec![],
                                                    qself: None,
                                                    path: rust_pat_variant_path,
                                                }),
                                                Some(_) => {
                                                    syn::Pat::TupleStruct(syn::PatTupleStruct {
                                                        attrs: vec![],
                                                        qself: None,
                                                        path: rust_pat_variant_path,
                                                        paren_token: syn::token::Paren(syn_span()),
                                                        elems: std::iter::once(syn_pat_variable(
                                                            variable_value_variable_name,
                                                        ))
                                                        .collect(),
                                                    })
                                                }
                                            },
                                            fat_arrow_token: syn::token::FatArrow(syn_span()),
                                            body: Box::new(match maybe_variant_value {
                                                None => rust_result_variant_constructor,
                                                Some(_) => syn::Expr::Call(syn::ExprCall {
                                                    attrs: vec![],
                                                    func: Box::new(rust_result_variant_constructor),
                                                    paren_token: syn::token::Paren(syn_span()),
                                                    args: std::iter::once(syn::Expr::Call(
                                                        syn::ExprCall {
                                                            attrs: vec![],
                                                            func: Box::new(syn_expr_reference([
                                                                "StillIntoOwned",
                                                                "into_owned",
                                                            ])),
                                                            args: std::iter::once(
                                                                syn_expr_reference([
                                                                    variable_value_variable_name,
                                                                ]),
                                                            )
                                                            .collect(),
                                                            paren_token: syn::token::Paren(
                                                                syn_span(),
                                                            ),
                                                        },
                                                    ))
                                                    .collect(),
                                                }),
                                            }),
                                            comma: Some(syn::token::Comma(syn_span())),
                                        }
                                    })
                                    .collect(),
                            }),
                            None,
                        )],
                    },
                }),
            ],
        });
        let impl_owned_to_still: syn::Item = syn::Item::Impl(syn::ItemImpl {
            attrs: vec![],
            defaultness: None,
            unsafety: None,
            impl_token: syn::token::Impl(syn_span()),
            generics: syn::Generics {
                lt_token: Some(syn::token::Lt(syn_span())),
                params: parameters
                    .iter()
                    .map(|parameter_node| {
                        syn::GenericParam::Type(syn::TypeParam {
                            attrs: vec![],
                            ident: syn_ident(&still_type_variable_to_rust(&parameter_node.value)),
                            colon_token: Some(syn::token::Colon(syn_span())),
                            bounds: [
                                syn::TypeParamBound::Trait(syn::TraitBound {
                                    paren_token: None,
                                    modifier: syn::TraitBoundModifier::None,
                                    lifetimes: None,
                                    path: syn::Path::from(syn_ident("StillIntoOwned")),
                                }),
                                // because still vec requires Clone for into_owned
                                syn::TypeParamBound::Trait(syn::TraitBound {
                                    paren_token: None,
                                    modifier: syn::TraitBoundModifier::None,
                                    lifetimes: None,
                                    path: syn::Path::from(syn_ident("Clone")),
                                }),
                            ]
                            .into_iter()
                            .collect(),
                            eq_token: None,
                            default: None,
                        })
                    })
                    .collect(),
                gt_token: Some(syn::token::Gt(syn_span())),
                where_clause: Some(syn::WhereClause {
                    where_token: syn::token::Where(syn_span()),
                    predicates: parameters
                        .iter()
                        .map(|parameter_node| {
                            syn::WherePredicate::Type(syn::PredicateType {
                                lifetimes: None,
                                bounded_ty: syn::Type::Path(syn::TypePath {
                                    qself: None,
                                    path: syn_path_reference([
                                        &still_type_variable_to_rust(&parameter_node.value),
                                        "Owned",
                                    ]),
                                }),
                                colon_token: syn::token::Colon(syn_span()),
                                bounds: std::iter::once(syn::TypeParamBound::Trait(syn::TraitBound {
                                    paren_token: None,
                                    modifier: syn::TraitBoundModifier::None,
                                    lifetimes: None,
                                    path: syn::Path::from(syn_ident("OwnedToStill")),
                                }))
                                .collect(),
                            })
                        })
                        .collect(),
                }),
            },
            trait_: Some((
                None,
                syn_path_reference(["OwnedToStill"]),
                syn::token::For(syn_span()),
            )),
            self_ty: Box::new(syn::Type::Path(syn::TypePath {
                qself: None,
                path: syn_path_name_with_arguments(
                    &owned_rust_enum_name,
                    parameters.iter().map(|parameter_node| {
                        syn::GenericArgument::Type(syn::Type::Path(syn::TypePath {
                            qself: None,
                            path: syn_path_reference([&still_type_variable_to_rust(
                                &parameter_node.value,
                            )]),
                        }))
                    }),
                ),
            })),
            brace_token: syn::token::Brace(syn_span()),
            items: vec![
                syn::ImplItem::Type(syn::ImplItemType {
                    attrs: vec![],
                    vis: syn::Visibility::Inherited,
                    defaultness: None,
                    type_token: syn::token::Type(syn_span()),
                    ident: syn_ident("Still"),
                    generics: syn::Generics {
                        lt_token: Some(syn::token::Lt(syn_span())),
                        params: std::iter::once(syn::GenericParam::Lifetime(
                            syn_default_lifetime_param(),
                        ))
                        .collect(),
                        gt_token: Some(syn::token::Gt(syn_span())),
                        where_clause: Some(syn::WhereClause {
                            where_token: syn::token::Where(syn_span()),
                            predicates: parameters
                                .iter()
                                .map(|parameter_node| {
                                    syn::WherePredicate::Type(syn::PredicateType {
                                        lifetimes: None,
                                        bounded_ty: syn::Type::Path(syn::TypePath {
                                            qself: None,
                                            path: syn_path_reference([&still_type_variable_to_rust(
                                                &parameter_node.value,
                                            )]),
                                        }),
                                        colon_token: syn::token::Colon(syn_span()),
                                        bounds: std::iter::once(syn::TypeParamBound::Lifetime(
                                            syn_default_lifetime(),
                                        ))
                                        .collect(),
                                    })
                                })
                                .collect(),
                        }),
                    },
                    eq_token: syn::token::Eq(syn_span()),
                    ty: syn::Type::Path(syn::TypePath {
                        qself: None,
                        path: syn_path_name_with_arguments(
                            &rust_enum_name,
                            (if has_lifetime_parameter {
                                Some(syn::GenericArgument::Lifetime(syn_default_lifetime()))
                            } else {
                                None
                            })
                            .into_iter()
                            .chain(parameters.iter().map(|parameter_node| {
                                syn::GenericArgument::Type(syn::Type::Path(syn::TypePath {
                                    qself: Some(syn::QSelf {
                                        lt_token: syn::token::Lt(syn_span()),
                                        ty: Box::new(syn::Type::Path(syn::TypePath {
                                            qself: None,
                                            path: syn_path_reference([
                                                &still_type_variable_to_rust(&parameter_node.value),
                                                "Owned",
                                            ]),
                                        })),
                                        position: 1,
                                        gt_token: syn::token::Gt(syn_span()),
                                        as_token: Some(syn::token::As(syn_span())),
                                    }),
                                    path: syn::Path {
                                        leading_colon: None,
                                        segments: [
                                            syn_path_segment_ident("OwnedToStill"),
                                            syn::PathSegment {
                                                ident: syn_ident("Still"),
                                                arguments: syn::PathArguments::AngleBracketed(
                                                    syn::AngleBracketedGenericArguments {
                                                        colon2_token: None,
                                                        lt_token: syn::token::Lt(syn_span()),
                                                        args: std::iter::once(
                                                            syn::GenericArgument::Lifetime(
                                                                syn_default_lifetime(),
                                                            ),
                                                        )
                                                        .collect(),
                                                        gt_token: syn::token::Gt(syn_span()),
                                                    },
                                                ),
                                            },
                                        ]
                                        .into_iter()
                                        .collect(),
                                    },
                                }))
                            })),
                        ),
                    }),
                    semi_token: syn::token::Semi(syn_span()),
                }),
                syn::ImplItem::Fn(syn::ImplItemFn {
                    attrs: vec![],
                    vis: syn::Visibility::Inherited,
                    defaultness: None,
                    sig: syn::Signature {
                        constness: None,
                        asyncness: None,
                        unsafety: None,
                        abi: None,
                        fn_token: syn::token::Fn(syn_span()),
                        ident: syn_ident("to_still"),
                        generics: syn::Generics {
                            lt_token: Some(syn::token::Lt(syn_span())),
                            params: std::iter::once(syn::GenericParam::Lifetime(
                                syn_default_lifetime_param(),
                            ))
                            .collect(),
                            gt_token: Some(syn::token::Gt(syn_span())),
                            where_clause: None,
                        },
                        paren_token: syn::token::Paren(syn_span()),
                        inputs: [
                            syn::FnArg::Receiver(syn::Receiver {
                                attrs: vec![],
                                reference: Some((
                                    syn::token::And(syn_span()),
                                    Some(syn_default_lifetime()),
                                )),
                                mutability: None,
                                self_token: syn::token::SelfValue(syn_span()),
                                colon_token: None,
                                ty: Box::new(syn::Type::Reference(syn::TypeReference {
                                    and_token: syn::token::And(syn_span()),
                                    lifetime: Some(syn_default_lifetime()),
                                    mutability: None,
                                    elem: Box::new(syn::Type::Path(syn::TypePath {
                                        qself: None,
                                        path: syn_path_reference(["Self"]),
                                    })),
                                })),
                            }),
                            default_allocator_fn_arg(),
                        ]
                        .into_iter()
                        .collect(),
                        variadic: None,
                        output: syn::ReturnType::Type(
                            syn::token::RArrow(syn_span()),
                            Box::new(syn::Type::Path(syn::TypePath {
                                qself: None,
                                path: syn::Path {
                                    leading_colon: None,
                                    segments: [
                                        syn_path_segment_ident("Self"),
                                        syn::PathSegment {
                                            ident: syn_ident("Still"),
                                            arguments: syn::PathArguments::AngleBracketed(
                                                syn::AngleBracketedGenericArguments {
                                                    colon2_token: None,
                                                    lt_token: syn::token::Lt(syn_span()),
                                                    args: std::iter::once(
                                                        syn::GenericArgument::Lifetime(
                                                            syn_default_lifetime(),
                                                        ),
                                                    )
                                                    .collect(),
                                                    gt_token: syn::token::Gt(syn_span()),
                                                },
                                            ),
                                        },
                                    ]
                                    .into_iter()
                                    .collect(),
                                },
                            })),
                        ),
                    },
                    block: syn::Block {
                        brace_token: syn::token::Brace(syn_span()),
                        stmts: vec![syn::Stmt::Expr(
                            syn::Expr::Match(syn::ExprMatch {
                                attrs: vec![],
                                match_token: syn::token::Match(syn_span()),
                                brace_token: syn::token::Brace(syn_span()),
                                expr: Box::new(syn_expr_reference(["self"])),
                                arms: variants
                                    .iter()
                                    .filter_map(|variant| {
                                        variant.name.as_ref().map(|n| {
                                            (
                                                still_syntax_node_as_ref_map(n, StillName::as_str),
                                                variant.value.as_ref().map(still_syntax_node_as_ref),
                                            )
                                        })
                                    })
                                    .map(|(variant_name, maybe_variant_value)| {
                                        let rust_variant_name: String =
                                            still_name_to_uppercase_rust(variant_name.value);
                                        let rust_pat_variant_path = syn_path_reference([
                                            &owned_rust_enum_name,
                                            &rust_variant_name,
                                        ]);
                                        let rust_result_variant_constructor =
                                            syn_expr_reference([&rust_enum_name, &rust_variant_name]);
                                        syn::Arm {
                                            attrs: vec![],
                                            guard: None,
                                            pat: match maybe_variant_value {
                                                None => syn::Pat::Path(syn::PatPath {
                                                    attrs: vec![],
                                                    qself: None,
                                                    path: rust_pat_variant_path,
                                                }),
                                                Some(_) => syn::Pat::TupleStruct(syn::PatTupleStruct {
                                                    attrs: vec![],
                                                    qself: None,
                                                    path: rust_pat_variant_path,
                                                    paren_token: syn::token::Paren(syn_span()),
                                                    elems: std::iter::once(syn_pat_variable(
                                                        variable_value_variable_name,
                                                    ))
                                                    .collect(),
                                                }),
                                            },
                                            fat_arrow_token: syn::token::FatArrow(syn_span()),
                                            body: Box::new(match maybe_variant_value {
                                                None => rust_result_variant_constructor,
                                                Some(_) => syn::Expr::Call(syn::ExprCall {
                                                    attrs: vec![],
                                                    func: Box::new(rust_result_variant_constructor),
                                                    paren_token: syn::token::Paren(syn_span()),
                                                    args: std::iter::once(syn::Expr::Call(
                                                        syn::ExprCall {
                                                            attrs: vec![],
                                                            func: Box::new(syn_expr_reference([
                                                                "OwnedToStill",
                                                                "to_still",
                                                            ])),
                                                            paren_token: syn::token::Paren(syn_span()),
                                                            args: [
                                                                syn_expr_reference([
                                                                    variable_value_variable_name,
                                                                ]),
                                                                syn_expr_reference([
                                                                    default_allocator_parameter_name,
                                                                ]),
                                                            ]
                                                            .into_iter()
                                                            .collect(),
                                                        },
                                                    ))
                                                    .collect(),
                                                }),
                                            }),
                                            comma: Some(syn::token::Comma(syn_span())),
                                        }
                                    })
                                    .collect(),
                            }),
                            None,
                        )],
                    },
                }),
            ],
        });
        rust_items.extend([rust_owned_enum, impl_owned_to_still, impl_still_to_owned]);
    }
    Some(CompiledRustChoiceTypeInfo {
        is_copy: is_copy,
        has_owned_representation: has_owned_representation,
        has_lifetime_parameter: has_lifetime_parameter,
        variants: type_variants,
    })
}
fn still_type_is_copy(
    variables_are_copy: bool,
    type_aliases: &std::collections::HashMap<StillName, TypeAliasInfo>,
    choice_types: &std::collections::HashMap<StillName, ChoiceTypeInfo>,
    type_: &StillType,
) -> bool {
    match type_ {
        StillType::Variable(_) => variables_are_copy,
        StillType::Function { .. } => {
            true
            // TODO for non-dyn it would be false
        }
        StillType::ChoiceConstruct {
            name: name_node,
            arguments,
        } => {
            (match choice_types.get(name_node.as_str()) {
                None => {
                    match type_aliases.get(name_node.as_str()) {
                        None => {
                            // not found, therefore from (mutually) recursive type,
                            // therefore compiled to a reference, therefore Copy
                            true
                        }
                        Some(compile_type_alias_info) => compile_type_alias_info.is_copy,
                    }
                }
                Some(choice_type_info) => choice_type_info.is_copy,
            }) && arguments.iter().all(|input_type| {
                still_type_is_copy(variables_are_copy, type_aliases, choice_types, input_type)
            })
        }
        StillType::Record(fields) => fields.iter().all(|field| {
            still_type_is_copy(variables_are_copy, type_aliases, choice_types, &field.value)
        }),
    }
}
/// TODO make part of `still_type_to_type`
fn still_type_uses_lifetime(
    type_aliases: &std::collections::HashMap<StillName, TypeAliasInfo>,
    choice_types: &std::collections::HashMap<StillName, ChoiceTypeInfo>,
    type_: &StillType,
) -> bool {
    match type_ {
        StillType::Variable(_) => false,
        StillType::Function { .. } => true,
        StillType::ChoiceConstruct { name, arguments } => {
            (match type_aliases.get(name.as_str()) {
                None => {
                    match choice_types.get(name.as_str()) {
                        None => {
                            // not found, therefore from (mutually) recursive type,
                            // therefore compiled to a reference
                            true
                        }
                        Some(choice_type_info) => choice_type_info.has_lifetime_parameter,
                    }
                }
                Some(type_alias) => type_alias.has_lifetime_parameter,
            }) && arguments
                .iter()
                .all(|input_type| still_type_uses_lifetime(type_aliases, choice_types, input_type))
        }
        StillType::Record(fields) => fields
            .iter()
            .all(|field| still_type_uses_lifetime(type_aliases, choice_types, &field.value)),
    }
}
/// TODO make part of `still_type_to_type`
fn still_type_has_owned_representation(
    variables_have_owned_representation: bool,
    type_aliases: &std::collections::HashMap<StillName, TypeAliasInfo>,
    choice_types: &std::collections::HashMap<StillName, ChoiceTypeInfo>,
    type_: &StillType,
) -> bool {
    match type_ {
        StillType::Variable(_) => variables_have_owned_representation,
        StillType::Function { .. } => false,
        StillType::ChoiceConstruct {
            name: name_node,
            arguments,
        } => {
            (match choice_types.get(name_node.as_str()) {
                None => {
                    match type_aliases.get(name_node.as_str()) {
                        None => {
                            // not found, therefore from (mutually) recursive type,
                            // therefore compiled to a reference, therefore Copy
                            true
                        }
                        Some(compile_type_alias_info) => {
                            compile_type_alias_info.has_owned_representation
                        }
                    }
                }
                Some(choice_type_info) => choice_type_info.has_owned_representation,
            }) && arguments.iter().all(|input_type| {
                still_type_has_owned_representation(
                    variables_have_owned_representation,
                    type_aliases,
                    choice_types,
                    input_type,
                )
            })
        }
        StillType::Record(fields) => fields.iter().all(|field| {
            still_type_has_owned_representation(
                variables_have_owned_representation,
                type_aliases,
                choice_types,
                &field.value,
            )
        }),
    }
}
/// TODO merge into `still_syntax_type_to_type`
fn still_type_constructs_recursive_type_in(
    scc_type_declaration_names: &std::collections::HashSet<&str>,
    type_: &StillType,
) -> bool {
    match type_ {
        StillType::Variable(_) => false,
        StillType::Function { inputs, output } => {
            still_type_constructs_recursive_type_in(scc_type_declaration_names, output)
                || (inputs.iter().any(|input_type| {
                    still_type_constructs_recursive_type_in(scc_type_declaration_names, input_type)
                }))
        }
        StillType::ChoiceConstruct { name, arguments } => {
            // skipped for now as recursive types are currently assumed to always contain a lifetime
            // if name_node.value == still_type_vec_name {
            //     // is already behind a reference
            //     false
            // } else
            //
            // more precise would be expanding type aliases here and checking the result
            // (to cover e.g. type alias list A = vec A).
            // skipped for now for performance
            scc_type_declaration_names.contains(name.as_str())
                || (arguments.iter().any(|argument_type| {
                    still_type_constructs_recursive_type_in(
                        scc_type_declaration_names,
                        argument_type,
                    )
                }))
        }
        StillType::Record(fields) => fields.iter().any(|field| {
            still_type_constructs_recursive_type_in(scc_type_declaration_names, &field.value)
        }),
    }
}
/// second result is `has_allocator_parameter` (TODO make a struct)
struct CompiledVariableDeclaration {
    rust: syn::Item,
    has_allocator_parameter: bool,
    type_: StillType,
    kind: RustVariableItemKind,
}
#[derive(Clone, Copy)]
enum RustVariableItemKind {
    Fn,
    Static,
}
fn variable_declaration_to_rust<'a>(
    errors: &mut Vec<StillErrorNode>,
    records_used: &mut std::collections::HashSet<Vec<StillName>>,
    type_aliases: &std::collections::HashMap<StillName, TypeAliasInfo>,
    choice_types: &std::collections::HashMap<StillName, ChoiceTypeInfo>,
    variable_declarations: &std::collections::HashMap<StillName, CompiledVariableDeclarationInfo>,
    variable_declaration_info: StillSyntaxVariableDeclarationInfo<'a>,
) -> Option<CompiledVariableDeclaration> {
    let Some(result_node) = variable_declaration_info.result else {
        errors.push(StillErrorNode {
            range: variable_declaration_info.range,
            message: Box::from(
                "missing expression after the variable declaration name ..variable-name.. here",
            ),
        });
        return None;
    };
    let compiled_result: CompiledStillExpression = still_syntax_expression_to_rust(
        errors,
        records_used,
        type_aliases,
        choice_types,
        variable_declarations,
        std::rc::Rc::new(std::collections::HashMap::new()),
        FnRepresentation::Impl,
        result_node,
    );
    let Some(type_) = compiled_result.type_ else {
        // rust top level declarations need explicit types; partial types won't do
        return None;
    };
    let mut still_type_parameters: std::collections::HashSet<&str> =
        std::collections::HashSet::new();
    still_type_variables_into(&mut still_type_parameters, &type_);
    let rust_attrs: Vec<syn::Attribute> = variable_declaration_info
        .documentation
        .map(|n| syn_attribute_doc(&n.value))
        .into_iter()
        .collect::<Vec<_>>();
    let rust_ident: syn::Ident = syn_ident(&still_name_to_lowercase_rust(
        &variable_declaration_info.name.value,
    ));
    let has_lifetime_parameter: bool = compiled_result.uses_allocator
        || still_type_uses_lifetime(type_aliases, choice_types, &type_);
    let rust_generics: syn::Generics = syn::Generics {
        lt_token: Some(syn::token::Lt(syn_span())),
        params: (if has_lifetime_parameter {
            Some(syn::GenericParam::Lifetime(syn_default_lifetime_param()))
        } else {
            None
        })
        .into_iter()
        .chain(still_type_parameters.iter().map(|name| {
            syn::GenericParam::Type(syn::TypeParam {
                attrs: vec![],
                ident: syn_ident(&still_type_variable_to_rust(name)),
                colon_token: Some(syn::token::Colon(syn_span())),
                bounds: default_parameter_bounds(syn_default_lifetime_name).collect(),
                eq_token: None,
                default: None,
            })
        }))
        .collect(),
        gt_token: Some(syn::token::Gt(syn_span())),
        where_clause: None,
    };
    let has_no_type_variable_parameters: bool = still_type_parameters.is_empty();
    match type_ {
        StillType::Function {
            inputs: input_types,
            output,
        } => match compiled_result.rust {
            syn::Expr::Closure(result_lambda) => {
                let rust_parameters: syn::punctuated::Punctuated<syn::FnArg, syn::token::Comma> =
                    (if compiled_result.uses_allocator {
                        Some(default_allocator_fn_arg())
                    } else {
                        None
                    })
                    .into_iter()
                    .chain(
                        result_lambda
                            .inputs
                            .into_iter()
                            .zip(input_types.iter())
                            .map(|(parameter_pat, parameter_type)| {
                                syn::FnArg::Typed(syn::PatType {
                                    pat: Box::new(parameter_pat),
                                    attrs: vec![],
                                    colon_token: syn::token::Colon(syn_span()),
                                    ty: Box::new(still_type_to_rust(
                                        type_aliases,
                                        choice_types,
                                        syn_default_lifetime_name,
                                        FnRepresentation::Impl,
                                        parameter_type,
                                    )),
                                })
                            }),
                    )
                    .collect();
                Some(CompiledVariableDeclaration {
                    rust: (syn::Item::Fn(syn::ItemFn {
                        attrs: rust_attrs,
                        vis: syn::Visibility::Public(syn::token::Pub(syn_span())),
                        sig: syn::Signature {
                            constness: None,
                            asyncness: None,
                            unsafety: None,
                            abi: None,
                            fn_token: syn::token::Fn(syn_span()),
                            ident: rust_ident,
                            generics: rust_generics,
                            paren_token: syn::token::Paren(syn_span()),
                            inputs: rust_parameters,
                            output: syn::ReturnType::Type(
                                syn::token::RArrow(syn_span()),
                                Box::new(still_type_to_rust(
                                    type_aliases,
                                    choice_types,
                                    syn_default_lifetime_name,
                                    FnRepresentation::Impl,
                                    &output,
                                )),
                            ),
                            variadic: None,
                        },
                        block: Box::new(syn_spread_expr_block(*result_lambda.body)),
                    })),
                    has_allocator_parameter: compiled_result.uses_allocator,
                    type_: StillType::Function {
                        inputs: input_types,
                        output,
                    },
                    kind: RustVariableItemKind::Fn,
                })
            }
            result_rust => Some(CompiledVariableDeclaration {
                rust: syn::Item::Fn(syn::ItemFn {
                    attrs: rust_attrs,
                    vis: syn::Visibility::Public(syn::token::Pub(syn_span())),
                    sig: syn::Signature {
                        constness: None,
                        asyncness: None,
                        unsafety: None,
                        abi: None,
                        fn_token: syn::token::Fn(syn_span()),
                        ident: rust_ident,
                        generics: rust_generics,
                        paren_token: syn::token::Paren(syn_span()),
                        inputs: (if compiled_result.uses_allocator {
                            Some(default_allocator_fn_arg())
                        } else {
                            None
                        })
                        .into_iter()
                        .chain(input_types.iter().enumerate().map(|(i, input_type_node)| {
                            syn::FnArg::Typed(syn::PatType {
                                pat: Box::new(syn::Pat::Path(syn::ExprPath {
                                    attrs: vec![],
                                    qself: None,
                                    path: syn_path_reference([&rust_generated_fn_parameter_name(
                                        i,
                                    )]),
                                })),
                                attrs: vec![],
                                colon_token: syn::token::Colon(syn_span()),
                                ty: Box::new(still_type_to_rust(
                                    type_aliases,
                                    choice_types,
                                    syn_default_lifetime_name,
                                    FnRepresentation::Impl,
                                    input_type_node,
                                )),
                            })
                        }))
                        .collect(),
                        output: syn::ReturnType::Type(
                            syn::token::RArrow(syn_span()),
                            Box::new(still_type_to_rust(
                                type_aliases,
                                choice_types,
                                syn_default_lifetime_name,
                                FnRepresentation::Impl,
                                &output,
                            )),
                        ),
                        variadic: None,
                    },
                    block: Box::new(syn::Block {
                        brace_token: syn::token::Brace(syn_span()),
                        stmts: vec![syn::Stmt::Expr(
                            syn::Expr::Call(syn::ExprCall {
                                attrs: vec![],
                                func: Box::new(result_rust),
                                paren_token: syn::token::Paren(syn_span()),
                                args: input_types
                                    .iter()
                                    .enumerate()
                                    .map(|(i, _)| {
                                        syn::Expr::Path(syn::ExprPath {
                                            attrs: vec![],
                                            qself: None,
                                            path: syn_path_reference([
                                                &rust_generated_fn_parameter_name(i),
                                            ]),
                                        })
                                    })
                                    .collect(),
                            }),
                            None,
                        )],
                    }),
                }),
                has_allocator_parameter: compiled_result.uses_allocator,
                type_: StillType::Function {
                    inputs: input_types,
                    output,
                },
                kind: RustVariableItemKind::Fn,
            }),
        },
        type_not_function => {
            if has_no_type_variable_parameters
                // not necessary: && !has_lifetime_parameter
                && // only covers a subset of theoretical rust values that qualify,
                   // and doesn't allow e.g. Box<str>.
                   // It only coincidentally works for all current still values.
                   still_type_is_copy(false, type_aliases, choice_types, &type_not_function)
            {
                Some(CompiledVariableDeclaration {
                    rust: syn::Item::Static(syn::ItemStatic {
                        attrs: rust_attrs,
                        vis: syn::Visibility::Public(syn::token::Pub(syn_span())),
                        mutability: syn::StaticMutability::None,
                        static_token: syn::token::Static(syn_span()),
                        ident: rust_ident,
                        colon_token: syn::token::Colon(syn_span()),
                        ty: Box::new(still_type_to_rust(
                            type_aliases,
                            choice_types,
                            syn_static_lifetime_name,
                            FnRepresentation::RefDyn,
                            &type_not_function,
                        )),
                        eq_token: syn::token::Eq(syn_span()),
                        expr: Box::new(compiled_result.rust),
                        semi_token: syn::token::Semi(syn_span()),
                    }),
                    has_allocator_parameter: false,
                    type_: type_not_function,
                    kind: RustVariableItemKind::Static,
                })
            } else {
                Some(CompiledVariableDeclaration {
                    rust: syn::Item::Fn(syn::ItemFn {
                        attrs: rust_attrs,
                        vis: syn::Visibility::Public(syn::token::Pub(syn_span())),
                        sig: syn::Signature {
                            constness: None,
                            asyncness: None,
                            unsafety: None,
                            abi: None,
                            fn_token: syn::token::Fn(syn_span()),
                            ident: rust_ident,
                            generics: rust_generics,
                            paren_token: syn::token::Paren(syn_span()),
                            inputs: (if compiled_result.uses_allocator {
                                Some(default_allocator_fn_arg())
                            } else {
                                None
                            })
                            .into_iter()
                            .collect(),
                            output: syn::ReturnType::Type(
                                syn::token::RArrow(syn_span()),
                                Box::new(still_type_to_rust(
                                    type_aliases,
                                    choice_types,
                                    syn_default_lifetime_name,
                                    FnRepresentation::Impl,
                                    &type_not_function,
                                )),
                            ),
                            variadic: None,
                        },
                        block: Box::new(syn_spread_expr_block(compiled_result.rust)),
                    }),
                    has_allocator_parameter: compiled_result.uses_allocator,
                    type_: type_not_function,
                    kind: RustVariableItemKind::Fn,
                })
            }
        }
    }
}
fn syn_spread_expr_block(syn_expr: syn::Expr) -> syn::Block {
    match syn_expr {
        syn::Expr::Block(block) => block.block,
        _ => syn::Block {
            brace_token: syn::token::Brace(syn_span()),
            stmts: vec![syn::Stmt::Expr(syn_expr, None)],
        },
    }
}
fn rust_generated_fn_parameter_name(index: usize) -> String {
    format!("parameter·{index}")
}
fn still_syntax_type_to_function(
    type_aliases: &std::collections::HashMap<StillName, TypeAliasInfo>,
    still_type_node: StillSyntaxNode<&StillSyntaxType>,
) -> Option<(
    Vec<StillSyntaxNode<StillSyntaxType>>,
    Option<StillSyntaxNode<StillSyntaxType>>,
)> {
    match still_type_node.value {
        StillSyntaxType::Function {
            inputs,
            arrow_key_symbol_range: _,
            output: maybe_output,
        } => Some((
            inputs.clone(),
            maybe_output
                .as_ref()
                .map(|n| still_syntax_node_as_ref_map(n, |v| v.as_ref().clone())),
        )),
        StillSyntaxType::WithComment {
            comment: _,
            type_: Some(after_comment_node),
        } => {
            still_syntax_type_to_function(type_aliases, still_syntax_node_unbox(after_comment_node))
        }
        StillSyntaxType::Parenthesized(Some(in_parens_node)) => {
            still_syntax_type_to_function(type_aliases, still_syntax_node_unbox(in_parens_node))
        }
        _ => match still_syntax_type_resolve_while_type_alias(type_aliases, still_type_node) {
            None => None,
            Some(resolved) => {
                still_syntax_type_to_function(type_aliases, still_syntax_node_as_ref(&resolved))
            }
        },
    }
}
fn still_syntax_type_to_record(
    type_aliases: &std::collections::HashMap<StillName, TypeAliasInfo>,
    still_type_node: StillSyntaxNode<&StillSyntaxType>,
) -> Option<Vec<StillSyntaxTypeField>> {
    match still_type_node.value {
        StillSyntaxType::Record(fields) => Some(fields.clone()),
        StillSyntaxType::WithComment {
            comment: _,
            type_: Some(after_comment_node),
        } => still_syntax_type_to_record(type_aliases, still_syntax_node_unbox(after_comment_node)),
        StillSyntaxType::Parenthesized(Some(in_parens_node)) => {
            still_syntax_type_to_record(type_aliases, still_syntax_node_unbox(in_parens_node))
        }
        _ => match still_syntax_type_resolve_while_type_alias(type_aliases, still_type_node) {
            None => None,
            Some(resolved) => {
                still_syntax_type_to_record(type_aliases, still_syntax_node_as_ref(&resolved))
            }
        },
    }
}
fn still_syntax_type_to_choice_type(
    type_aliases: &std::collections::HashMap<StillName, TypeAliasInfo>,
    still_type_node: StillSyntaxNode<&StillSyntaxType>,
) -> Option<(Box<str>, Vec<StillSyntaxNode<StillSyntaxType>>)> {
    match still_type_node.value {
        StillSyntaxType::WithComment {
            comment: _,
            type_: Some(after_comment_node),
        } => still_syntax_type_to_choice_type(
            type_aliases,
            still_syntax_node_unbox(after_comment_node),
        ),
        StillSyntaxType::Parenthesized(Some(in_parens_node)) => {
            still_syntax_type_to_choice_type(type_aliases, still_syntax_node_unbox(in_parens_node))
        }
        StillSyntaxType::Construct {
            name: name_node,
            arguments,
        } => match still_syntax_type_resolve_while_type_alias(type_aliases, still_type_node) {
            None => Some((Box::from(name_node.value.as_str()), arguments.clone())),
            Some(resolved) => {
                still_syntax_type_to_choice_type(type_aliases, still_syntax_node_as_ref(&resolved))
            }
        },
        _ => None,
    }
}
fn still_type_construct_resolve_type_alias(
    origin_type_alias: &TypeAliasInfo,
    argument_types: &[StillType],
) -> Option<StillType> {
    let Some(type_alias_type) = &origin_type_alias.type_ else {
        return None;
    };
    if origin_type_alias.parameters.is_empty() {
        return Some(type_alias_type.clone());
    }
    let type_parameter_replacements: std::collections::HashMap<&str, &StillType> =
        origin_type_alias
            .parameters
            .iter()
            .map(|n| n.value.as_str())
            .zip(argument_types.iter())
            .collect::<std::collections::HashMap<_, _>>();
    let mut peeled: StillType = type_alias_type.clone();
    still_type_replace_variables(&type_parameter_replacements, &mut peeled);
    Some(peeled)
}
fn still_type_replace_variables(
    type_parameter_replacements: &std::collections::HashMap<&str, &StillType>,
    type_: &mut StillType,
) {
    match type_ {
        StillType::Variable(variable) => {
            if let Some(&replacement_type_node) = type_parameter_replacements.get(variable.as_str())
            {
                *type_ = replacement_type_node.clone();
            }
        }
        StillType::ChoiceConstruct { name: _, arguments } => {
            for argument_type in arguments {
                still_type_replace_variables(type_parameter_replacements, argument_type);
            }
        }
        StillType::Record(fields) => {
            for field in fields {
                still_type_replace_variables(type_parameter_replacements, &mut field.value);
            }
        }
        StillType::Function { inputs, output } => {
            for input_type in inputs {
                still_type_replace_variables(type_parameter_replacements, input_type);
            }
            still_type_replace_variables(type_parameter_replacements, output);
        }
    }
}
#[derive(Clone)]
struct TypeAliasInfo {
    name_range: Option<lsp_types::Range>,
    documentation: Option<Box<str>>,
    parameters: Vec<StillSyntaxNode<StillName>>,
    // TODO is trying to recover something from partial type syntax overkill?
    type_syntax: Option<StillSyntaxNode<StillSyntaxType>>,
    type_: Option<StillType>,
    is_copy: bool,
    has_owned_representation: bool,
    has_lifetime_parameter: bool,
}
#[derive(Clone)]
struct ChoiceTypeInfo {
    name_range: Option<lsp_types::Range>,
    documentation: Option<Box<str>>,
    parameters: Vec<StillSyntaxNode<StillName>>,
    variants: Vec<StillSyntaxChoiceTypeVariant>,
    type_variants: Vec<StillChoiceTypeVariantInfo>,
    is_copy: bool,
    has_owned_representation: bool,
    has_lifetime_parameter: bool,
}
/// Keep peeling until the type is not a type alias anymore.
/// _Inner_ type aliases in a sub-part will not be resolved.
/// This will also not check for aliases inside parenthesized types or after comments
fn still_syntax_type_resolve_while_type_alias(
    type_aliases: &std::collections::HashMap<StillName, TypeAliasInfo>,
    type_node: StillSyntaxNode<&StillSyntaxType>,
) -> Option<StillSyntaxNode<StillSyntaxType>> {
    match type_node.value {
        StillSyntaxType::Construct {
            name: name_node,
            arguments,
        } => match type_aliases.get(&name_node.value) {
            None => None,
            Some(type_alias) => match &type_alias.type_syntax {
                None => None,
                Some(type_alias_type_node) => {
                    if type_alias.parameters.is_empty() {
                        return Some(type_alias_type_node.clone());
                    }
                    let type_parameter_replacements: std::collections::HashMap<
                        &str,
                        StillSyntaxNode<&StillSyntaxType>,
                    > = type_alias
                        .parameters
                        .iter()
                        .map(|n| n.value.as_str())
                        .zip(arguments.iter().map(still_syntax_node_as_ref))
                        .collect::<std::collections::HashMap<_, _>>();
                    let peeled: StillSyntaxNode<StillSyntaxType> =
                        still_syntax_type_replace_variables(
                            &type_parameter_replacements,
                            still_syntax_node_as_ref(type_alias_type_node),
                        );
                    Some(
                        match still_syntax_type_resolve_while_type_alias(
                            type_aliases,
                            still_syntax_node_as_ref(&peeled),
                        ) {
                            None => peeled,
                            Some(fully_peeled) => fully_peeled,
                        },
                    )
                }
            },
        },
        _ => None,
    }
}
fn still_syntax_type_replace_variables(
    type_parameter_replacements: &std::collections::HashMap<
        &str,
        StillSyntaxNode<&StillSyntaxType>,
    >,
    type_node: StillSyntaxNode<&StillSyntaxType>,
) -> StillSyntaxNode<StillSyntaxType> {
    match type_node.value {
        StillSyntaxType::Variable(variable) => {
            match type_parameter_replacements.get(variable.as_str()) {
                None => still_syntax_node_map(type_node, StillSyntaxType::clone),
                Some(&replacement_type_node) => {
                    still_syntax_node_map(replacement_type_node, StillSyntaxType::clone)
                }
            }
        }
        StillSyntaxType::Parenthesized(maybe_in_parens) => match maybe_in_parens {
            None => still_syntax_node_map(type_node, StillSyntaxType::clone),
            Some(in_parens_node) => StillSyntaxNode {
                range: type_node.range,
                value: StillSyntaxType::Parenthesized(Some(still_syntax_node_box(
                    still_syntax_type_replace_variables(
                        type_parameter_replacements,
                        still_syntax_node_unbox(in_parens_node),
                    ),
                ))),
            },
        },
        StillSyntaxType::WithComment {
            comment: maybe_comment,
            type_: maybe_type,
        } => StillSyntaxNode {
            range: type_node.range,
            value: StillSyntaxType::WithComment {
                comment: maybe_comment.clone(),
                type_: maybe_type.as_ref().map(|after_comment_node| {
                    still_syntax_node_box(still_syntax_type_replace_variables(
                        type_parameter_replacements,
                        still_syntax_node_unbox(after_comment_node),
                    ))
                }),
            },
        },
        StillSyntaxType::Construct {
            name: name_node,
            arguments,
        } => StillSyntaxNode {
            range: type_node.range,
            value: StillSyntaxType::Construct {
                name: name_node.clone(),
                arguments: arguments
                    .iter()
                    .map(|argument_node| {
                        still_syntax_type_replace_variables(
                            type_parameter_replacements,
                            still_syntax_node_as_ref(argument_node),
                        )
                    })
                    .collect(),
            },
        },
        StillSyntaxType::Record(fields) => StillSyntaxNode {
            range: type_node.range,
            value: StillSyntaxType::Record(
                fields
                    .iter()
                    .map(|field| StillSyntaxTypeField {
                        name: field.name.clone(),
                        value: field.value.as_ref().map(|field_value_node| {
                            still_syntax_type_replace_variables(
                                type_parameter_replacements,
                                still_syntax_node_as_ref(field_value_node),
                            )
                        }),
                    })
                    .collect(),
            ),
        },
        StillSyntaxType::Function {
            inputs,
            arrow_key_symbol_range: maybe_arrow_key_symbol_range,
            output: maybe_output,
        } => StillSyntaxNode {
            range: type_node.range,
            value: StillSyntaxType::Function {
                inputs: inputs
                    .iter()
                    .map(|argument_node| {
                        still_syntax_type_replace_variables(
                            type_parameter_replacements,
                            still_syntax_node_as_ref(argument_node),
                        )
                    })
                    .collect(),
                arrow_key_symbol_range: *maybe_arrow_key_symbol_range,
                output: maybe_output.as_ref().map(|after_comment_node| {
                    still_syntax_node_box(still_syntax_type_replace_variables(
                        type_parameter_replacements,
                        still_syntax_node_unbox(after_comment_node),
                    ))
                }),
            },
        },
    }
}
fn still_type_collect_variables_that_are_concrete_into(
    type_parameter_replacements: &mut std::collections::HashMap<Box<str>, StillType>,
    type_with_variables: &StillType,
    concrete_type: StillType,
) {
    match type_with_variables {
        StillType::Variable(variable_name) => {
            type_parameter_replacements.insert(Box::from(variable_name.as_str()), concrete_type);
        }
        StillType::Function {
            inputs,
            output: output_type,
        } => {
            if let StillType::Function {
                inputs: concrete_function_inputs,
                output: concrete_function_output_type,
            } = concrete_type
            {
                for (input_type, concrete_input_type) in
                    inputs.iter().zip(concrete_function_inputs.into_iter())
                {
                    still_type_collect_variables_that_are_concrete_into(
                        type_parameter_replacements,
                        input_type,
                        concrete_input_type,
                    );
                }
                still_type_collect_variables_that_are_concrete_into(
                    type_parameter_replacements,
                    output_type,
                    *concrete_function_output_type,
                );
            }
        }
        StillType::ChoiceConstruct { name, arguments } => {
            if let StillType::ChoiceConstruct {
                name: concrete_choice_type_construct_name,
                arguments: concrete_choice_type_construct_arguments,
            } = concrete_type
                && name == concrete_choice_type_construct_name
            {
                for (argument_type, concrete_argument_type) in arguments
                    .iter()
                    .zip(concrete_choice_type_construct_arguments.into_iter())
                {
                    still_type_collect_variables_that_are_concrete_into(
                        type_parameter_replacements,
                        argument_type,
                        concrete_argument_type,
                    );
                }
            }
        }
        StillType::Record(fields) => {
            if let StillType::Record(mut concrete_fields) = concrete_type {
                for field in fields {
                    if let Some(matching_concrete_field_index) = concrete_fields
                        .iter()
                        .position(|concrete_field| concrete_field.name == field.name)
                    {
                        let concrete_field =
                            concrete_fields.swap_remove(matching_concrete_field_index);
                        still_type_collect_variables_that_are_concrete_into(
                            type_parameter_replacements,
                            &field.value,
                            concrete_field.value,
                        );
                    }
                }
            }
        }
    }
}
fn still_syntax_type_collect_variables_that_are_concrete_into(
    type_parameter_replacements: &mut std::collections::HashMap<
        Box<str>,
        StillSyntaxNode<StillSyntaxType>,
    >,
    type_aliases: &std::collections::HashMap<StillName, TypeAliasInfo>,
    type_node_with_variables: StillSyntaxNode<&StillSyntaxType>,
    concrete_type_node: StillSyntaxNode<&StillSyntaxType>,
) {
    match type_node_with_variables.value {
        StillSyntaxType::Variable(variable_name) => {
            type_parameter_replacements.insert(
                Box::from(variable_name.as_str()),
                still_syntax_node_map(concrete_type_node, StillSyntaxType::clone),
            );
        }
        StillSyntaxType::Parenthesized(maybe_in_parens) => {
            if let Some(in_parens_node) = maybe_in_parens {
                still_syntax_type_collect_variables_that_are_concrete_into(
                    type_parameter_replacements,
                    type_aliases,
                    still_syntax_node_unbox(in_parens_node),
                    concrete_type_node,
                );
            }
        }
        StillSyntaxType::WithComment {
            comment: _,
            type_: maybe_after_comment,
        } => {
            if let Some(after_comment_node) = maybe_after_comment {
                still_syntax_type_collect_variables_that_are_concrete_into(
                    type_parameter_replacements,
                    type_aliases,
                    still_syntax_node_unbox(after_comment_node),
                    concrete_type_node,
                );
            }
        }
        StillSyntaxType::Function {
            inputs,
            arrow_key_symbol_range: _,
            output,
        } => {
            if let Some((concrete_function_inputs, concrete_function_maybe_output)) =
                still_syntax_type_to_function(type_aliases, concrete_type_node)
            {
                for (input_node, concrete_input_node) in
                    inputs.iter().zip(concrete_function_inputs.iter())
                {
                    still_syntax_type_collect_variables_that_are_concrete_into(
                        type_parameter_replacements,
                        type_aliases,
                        still_syntax_node_as_ref(input_node),
                        still_syntax_node_as_ref(concrete_input_node),
                    );
                }
                if let Some(output_node) = output
                    && let Some(concrete_output_node) = concrete_function_maybe_output
                {
                    still_syntax_type_collect_variables_that_are_concrete_into(
                        type_parameter_replacements,
                        type_aliases,
                        still_syntax_node_unbox(output_node),
                        still_syntax_node_as_ref(&concrete_output_node),
                    );
                }
            }
        }
        StillSyntaxType::Construct {
            name: name_node,
            arguments,
        } => {
            match still_syntax_type_resolve_while_type_alias(type_aliases, type_node_with_variables)
            {
                Some(resolved_type_node) => {
                    still_syntax_type_collect_variables_that_are_concrete_into(
                        type_parameter_replacements,
                        type_aliases,
                        still_syntax_node_as_ref(&resolved_type_node),
                        concrete_type_node,
                    );
                }
                None => {
                    if let Some((
                        concrete_choice_type_construct_name,
                        concrete_choice_type_construct_arguments,
                    )) = still_syntax_type_to_choice_type(type_aliases, concrete_type_node)
                        && name_node.value == concrete_choice_type_construct_name
                    {
                        for (argument_type_node, concrete_argument_type_node) in arguments
                            .iter()
                            .zip(concrete_choice_type_construct_arguments.iter())
                        {
                            still_syntax_type_collect_variables_that_are_concrete_into(
                                type_parameter_replacements,
                                type_aliases,
                                still_syntax_node_as_ref(argument_type_node),
                                still_syntax_node_as_ref(concrete_argument_type_node),
                            );
                        }
                    }
                }
            }
        }
        StillSyntaxType::Record(fields) => {
            if let Some(concrete_fields) =
                still_syntax_type_to_record(type_aliases, concrete_type_node)
            {
                for field in fields {
                    if let Some(field_value_node) = &field.value
                        && let Some(concrete_field_value_node) =
                            concrete_fields.iter().find_map(|concrete_field| {
                                if concrete_field.name.value == field.name.value {
                                    concrete_field.value.as_ref()
                                } else {
                                    None
                                }
                            })
                    {
                        still_syntax_type_collect_variables_that_are_concrete_into(
                            type_parameter_replacements,
                            type_aliases,
                            still_syntax_node_as_ref(field_value_node),
                            still_syntax_node_as_ref(concrete_field_value_node),
                        );
                    }
                }
            }
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum FnRepresentation {
    RefDyn,
    Impl,
}
fn still_type_to_rust(
    type_aliases: &std::collections::HashMap<StillName, TypeAliasInfo>,
    choice_types: &std::collections::HashMap<StillName, ChoiceTypeInfo>,
    lifetime: &str,
    fn_representation: FnRepresentation,
    type_: &StillType,
) -> syn::Type {
    match type_ {
        StillType::Variable(variable) => syn_type_variable(&still_type_variable_to_rust(variable)),
        StillType::Function { inputs, output } => {
            let output_rust_type: syn::Type = still_type_to_rust(
                type_aliases,
                choice_types,
                lifetime,
                FnRepresentation::RefDyn,
                output,
            );
            let fn_trait_bound: syn::TypeParamBound = syn::TypeParamBound::Trait(syn::TraitBound {
                paren_token: None,
                modifier: syn::TraitBoundModifier::None,
                lifetimes: None,
                path: syn::Path::from(syn::PathSegment {
                    ident: syn_ident("Fn"),
                    arguments: syn::PathArguments::Parenthesized(
                        syn::ParenthesizedGenericArguments {
                            paren_token: syn::token::Paren(syn_span()),
                            inputs: inputs
                                .iter()
                                .map(|input_type| {
                                    still_type_to_rust(
                                        type_aliases,
                                        choice_types,
                                        lifetime,
                                        FnRepresentation::RefDyn,
                                        input_type,
                                    )
                                })
                                .collect(),
                            output: syn::ReturnType::Type(
                                syn::token::RArrow(syn_span()),
                                Box::new(output_rust_type),
                            ),
                        },
                    ),
                }),
            });
            match fn_representation {
                FnRepresentation::Impl => syn::Type::ImplTrait(syn::TypeImplTrait {
                    impl_token: syn::token::Impl(syn_span()),
                    bounds: std::iter::once(fn_trait_bound)
                        .chain(default_parameter_bounds(lifetime))
                        .collect(),
                }),
                FnRepresentation::RefDyn => syn::Type::Reference(syn::TypeReference {
                    and_token: syn::token::And(syn_span()),
                    lifetime: Some(syn_lifetime(lifetime)),
                    mutability: None,
                    elem: Box::new(syn::Type::Paren(syn::TypeParen {
                        paren_token: syn::token::Paren(syn_span()),
                        elem: Box::new(syn::Type::TraitObject(syn::TypeTraitObject {
                            dyn_token: Some(syn::token::Dyn(syn_span())),
                            bounds: std::iter::once(fn_trait_bound)
                                .chain(default_dyn_fn_bounds(lifetime))
                                .collect(),
                        })),
                    })),
                }),
            }
        }
        StillType::ChoiceConstruct { name, arguments } => {
            let has_lifetime_parameter: bool = choice_types
                .get(name.as_str())
                .map(|compile_choice_type_info| compile_choice_type_info.has_lifetime_parameter)
                .or_else(|| {
                    type_aliases
                        .get(name.as_str())
                        .map(|compiled_type_alias| compiled_type_alias.has_lifetime_parameter)
                })
                .unwrap_or({
                    // recursive type → contains a reference with a lifetime
                    true
                });
            syn::Type::Path(syn::TypePath {
                qself: None,
                path: syn::Path {
                    leading_colon: None,
                    segments: std::iter::once(syn::PathSegment {
                        ident: syn_ident(&still_name_to_uppercase_rust(name)),
                        arguments: syn::PathArguments::AngleBracketed(
                            syn::AngleBracketedGenericArguments {
                                colon2_token: None,
                                lt_token: syn::token::Lt(syn_span()),
                                args: (if has_lifetime_parameter {
                                    Some(syn::GenericArgument::Lifetime(syn_lifetime(lifetime)))
                                } else {
                                    None
                                })
                                .into_iter()
                                .chain(arguments.iter().map(|argument_type| {
                                    syn::GenericArgument::Type(still_type_to_rust(
                                        type_aliases,
                                        choice_types,
                                        lifetime,
                                        fn_representation,
                                        argument_type,
                                    ))
                                }))
                                .collect(),
                                gt_token: syn::token::Gt(syn_span()),
                            },
                        ),
                    })
                    .collect(),
                },
            })
        }
        StillType::Record(fields) => {
            let mut fields_sorted: Vec<&StillTypeField> = fields.iter().collect();
            fields_sorted.sort_by(|a, b| {
                still_name_to_lowercase_rust(&a.name).cmp(&still_name_to_lowercase_rust(&b.name))
            });
            syn::Type::Path(syn::TypePath {
                qself: None,
                path: syn::Path {
                    leading_colon: None,
                    segments: std::iter::once(syn::PathSegment {
                        ident: syn_ident(&still_field_names_to_rust_record_struct_name(
                            fields_sorted.iter().map(|field| field.name.as_ref()),
                        )),
                        arguments: syn::PathArguments::AngleBracketed(
                            syn::AngleBracketedGenericArguments {
                                colon2_token: None,
                                lt_token: syn::token::Lt(syn_span()),
                                gt_token: syn::token::Gt(syn_span()),
                                args: fields_sorted
                                    .into_iter()
                                    .map(|field| {
                                        syn::GenericArgument::Type(still_type_to_rust(
                                            type_aliases,
                                            choice_types,
                                            lifetime,
                                            fn_representation,
                                            &field.value,
                                        ))
                                    })
                                    .collect(),
                            },
                        ),
                    })
                    .collect(),
                },
            })
        }
    }
}
fn still_type_variables_into<'a>(
    variables: &mut std::collections::HashSet<&'a str>,
    type_: &'a StillType,
) {
    match type_ {
        StillType::Variable(variable) => {
            variables.insert(variable);
        }
        StillType::Function { inputs, output } => {
            for input_type in inputs {
                still_type_variables_into(variables, input_type);
            }
            // TODO skip as it should not contain variables not used in the inputs
            still_type_variables_into(variables, output);
        }
        StillType::ChoiceConstruct { name: _, arguments } => {
            for argument_type in arguments {
                still_type_variables_into(variables, argument_type);
            }
        }
        StillType::Record(fields) => {
            for field in fields {
                still_type_variables_into(variables, &field.value);
            }
        }
    }
}
fn still_type_variables_and_records_into(
    type_variables: &mut std::collections::HashSet<StillName>,
    records_used: &mut std::collections::HashSet<Vec<StillName>>,
    type_: &StillType,
) {
    match type_ {
        StillType::Variable(name) => {
            type_variables.insert(name.clone());
        }
        StillType::Function { inputs, output } => {
            for input in inputs {
                still_type_variables_and_records_into(type_variables, records_used, input);
            }
            still_type_variables_and_records_into(type_variables, records_used, output);
        }
        StillType::ChoiceConstruct { name: _, arguments } => {
            for argument in arguments {
                still_type_variables_and_records_into(type_variables, records_used, argument);
            }
        }
        StillType::Record(fields) => {
            records_used.insert(sorted_field_names(fields.iter().map(|field| &field.name)));
            for field in fields {
                still_type_variables_and_records_into(type_variables, records_used, &field.value);
            }
        }
    }
}
struct CompiledStillExpression {
    rust: syn::Expr,
    uses_allocator: bool,
    type_: Option<StillType>,
}
fn maybe_still_syntax_expression_to_rust<'a>(
    errors: &mut Vec<StillErrorNode>,
    error_on_none: impl FnOnce() -> StillErrorNode,
    records_used: &mut std::collections::HashSet<Vec<StillName>>,
    type_aliases: &std::collections::HashMap<StillName, TypeAliasInfo>,
    choice_types: &std::collections::HashMap<StillName, ChoiceTypeInfo>,
    project_variable_declarations: &std::collections::HashMap<
        StillName,
        CompiledVariableDeclarationInfo,
    >,
    local_bindings: std::rc::Rc<std::collections::HashMap<&'a str, StillLocalBindingCompileInfo>>,
    closure_representation: FnRepresentation,
    maybe_expression: Option<StillSyntaxNode<&'a StillSyntaxExpression>>,
) -> CompiledStillExpression {
    match maybe_expression {
        None => {
            errors.push(error_on_none());
            CompiledStillExpression {
                rust: syn_expr_todo(),
                uses_allocator: false,
                type_: None,
            }
        }
        Some(expression_node) => still_syntax_expression_to_rust(
            errors,
            records_used,
            type_aliases,
            choice_types,
            project_variable_declarations,
            local_bindings,
            closure_representation,
            expression_node,
        ),
    }
}
// be aware: `last_uses` contains both variable ranges and closure ranges
#[derive(Clone, Debug)]
struct StillLocalBindingCompileInfo {
    origin_range: lsp_types::Range,
    type_: Option<StillType>,
    is_copy: bool,
    last_uses: Vec<lsp_types::Range>,
    closures_it_is_used_in: Vec<lsp_types::Range>,
}
fn still_syntax_expression_to_rust<'a>(
    errors: &mut Vec<StillErrorNode>,
    records_used: &mut std::collections::HashSet<Vec<StillName>>,
    type_aliases: &std::collections::HashMap<StillName, TypeAliasInfo>,
    choice_types: &std::collections::HashMap<StillName, ChoiceTypeInfo>,
    project_variable_declarations: &std::collections::HashMap<
        StillName,
        CompiledVariableDeclarationInfo,
    >,
    local_bindings: std::rc::Rc<std::collections::HashMap<&'a str, StillLocalBindingCompileInfo>>,
    closure_representation: FnRepresentation,
    expression_node: StillSyntaxNode<&'a StillSyntaxExpression>,
) -> CompiledStillExpression {
    match expression_node.value {
        StillSyntaxExpression::String {
            content,
            quoting_style: _,
        } => CompiledStillExpression {
            uses_allocator: false,
            rust: syn::Expr::Lit(syn::ExprLit {
                attrs: vec![],
                lit: syn::Lit::Str(syn::LitStr::new(content, syn_span())),
            }),
            type_: Some(still_type_str),
        },
        StillSyntaxExpression::Char(maybe_char) => CompiledStillExpression {
            uses_allocator: false,
            type_: Some(still_type_chr),
            rust: match *maybe_char {
                None => {
                    errors.push(StillErrorNode {
                        range: expression_node.range,
                        message: Box::from("missing character between 'here'"),
                    });
                    syn_expr_todo()
                }
                Some(char) => syn::Expr::Lit(syn::ExprLit {
                    attrs: vec![],
                    lit: syn::Lit::Char(syn::LitChar::new(char, syn_span())),
                }),
            },
        },
        StillSyntaxExpression::Dec(dec_or_err) => CompiledStillExpression {
            uses_allocator: false,
            type_: Some(still_type_dec),
            rust: match dec_or_err.parse::<f64>() {
                Err(parse_error) => {
                    errors.push(StillErrorNode {
                        range: expression_node.range,
                        message: Box::from(format!("dec literal cannot be parsed: {parse_error}")),
                    });
                    syn_expr_todo()
                }
                Ok(dec) => syn::Expr::Lit(syn::ExprLit {
                    attrs: vec![],
                    lit: syn::Lit::Float(syn::LitFloat::new(&dec.to_string(), syn_span())),
                }),
            },
        },
        StillSyntaxExpression::Int(representation) => CompiledStillExpression {
            uses_allocator: false,
            type_: Some(still_type_int),
            rust: match representation.parse::<isize>() {
                Err(parse_error) => {
                    errors.push(StillErrorNode {
                        range: expression_node.range,
                        message: Box::from(format!("int literal cannot be parsed: {parse_error}")),
                    });
                    syn_expr_todo()
                }
                Ok(int) => syn::Expr::Lit(syn::ExprLit {
                    attrs: vec![],
                    lit: syn::Lit::Int(syn::LitInt::new(&int.to_string(), syn_span())),
                }),
            },
        },
        StillSyntaxExpression::Lambda {
            parameters,
            arrow_key_symbol_range: maybe_arrow_key_symbol_range,
            result: maybe_lambda_result,
        } => {
            let mut parameter_introduced_bindings: std::collections::HashMap<
                &str,
                StillLocalBindingCompileInfo,
            > = std::collections::HashMap::new();
            let mut bindings_to_clone: Vec<BindingToClone> = Vec::new();
            let (rust_patterns, input_type_maybes): (
                syn::punctuated::Punctuated<syn::Pat, syn::token::Comma>,
                Vec<Option<StillType>>,
            ) = parameters
                .iter()
                .map(|parameter_node| {
                    let compiled_parameter: CompiledStillPattern = still_syntax_pattern_to_rust(
                        errors,
                        records_used,
                        &mut parameter_introduced_bindings,
                        &mut bindings_to_clone,
                        type_aliases,
                        choice_types,
                        false,
                        still_syntax_node_as_ref(parameter_node),
                    );
                    (
                        compiled_parameter.rust.unwrap_or_else(syn_pat_wild),
                        compiled_parameter.type_,
                    )
                })
                .collect();
            for (parameter_introduced_binding_name, parameter_introduced_binding_info) in
                &parameter_introduced_bindings
            {
                push_error_if_name_collides(
                    errors,
                    project_variable_declarations,
                    &local_bindings,
                    StillSyntaxNode {
                        range: parameter_introduced_binding_info.origin_range,
                        value: parameter_introduced_binding_name,
                    },
                );
            }
            if let Some(lambda_result_node) = maybe_lambda_result {
                still_syntax_expression_uses_of_local_bindings_into(
                    &mut parameter_introduced_bindings,
                    None,
                    still_syntax_node_unbox(lambda_result_node),
                );
            }
            let mut rust_clones_before_closure: Vec<syn::Stmt> = local_bindings
                .iter()
                .filter(|&(_, local_binding_info)| {
                    !local_binding_info.is_copy
                        && !local_binding_info
                            .last_uses
                            .contains(&expression_node.range)
                        && local_binding_info
                            .closures_it_is_used_in
                            .contains(&expression_node.range)
                })
                .map(|(&local_binding_name, _)| {
                    let introduced_local_binding_rust_name: String =
                        still_name_to_lowercase_rust(local_binding_name);
                    syn::Stmt::Local(syn::Local {
                        attrs: vec![],
                        let_token: syn::token::Let(syn_span()),
                        pat: syn::Pat::Ident(syn::PatIdent {
                            attrs: vec![],
                            by_ref: None,
                            mutability: None,
                            ident: syn_ident(&introduced_local_binding_rust_name),
                            subpat: None,
                        }),
                        init: Some(syn::LocalInit {
                            eq_token: syn::token::Eq(syn_span()),
                            expr: Box::new(syn_expr_call_clone_method(syn_expr_reference([
                                &introduced_local_binding_rust_name,
                            ]))),
                            diverge: None,
                        }),
                        semi_token: syn::token::Semi(syn_span()),
                    })
                })
                .collect();
            let mut local_bindings: std::collections::HashMap<&str, StillLocalBindingCompileInfo> =
                std::rc::Rc::unwrap_or_clone(local_bindings);
            local_bindings.extend(parameter_introduced_bindings);

            let mut closure_result_rust_stmts: Vec<syn::Stmt> = Vec::new();
            bindings_to_clone_to_rust_into(&mut closure_result_rust_stmts, bindings_to_clone);
            let compiled_result: CompiledStillExpression = maybe_still_syntax_expression_to_rust(
                errors,
                || match *maybe_arrow_key_symbol_range {
                    None => StillErrorNode {
                        range: expression_node.range,
                        message: Box::from(
                            "missing lambda arrow (>) and result after \\..parameters.. here",
                        ),
                    },
                    Some(arrow_key_symbol_range) => StillErrorNode {
                        range: arrow_key_symbol_range,
                        message: Box::from("missing lambda result after \\..parameters.. > here"),
                    },
                },
                records_used,
                type_aliases,
                choice_types,
                project_variable_declarations,
                std::rc::Rc::new(local_bindings),
                FnRepresentation::RefDyn,
                maybe_lambda_result.as_ref().map(still_syntax_node_unbox),
            );
            let rust_closure: syn::Expr = syn::Expr::Closure(syn::ExprClosure {
                attrs: vec![],
                lifetimes: None,
                constness: None,
                movability: None,
                asyncness: None,
                capture: Some(syn::token::Move(syn_span())),
                or1_token: syn::token::Or(syn_span()),
                inputs: rust_patterns,
                or2_token: syn::token::Or(syn_span()),
                output: syn::ReturnType::Default,
                body: Box::new(if closure_result_rust_stmts.is_empty() {
                    compiled_result.rust
                } else {
                    closure_result_rust_stmts.push(syn::Stmt::Expr(compiled_result.rust, None));
                    syn::Expr::Block(syn::ExprBlock {
                        attrs: vec![],
                        label: None,
                        block: syn::Block {
                            brace_token: syn::token::Brace(syn_span()),
                            stmts: closure_result_rust_stmts,
                        },
                    })
                }),
            });
            let maybe_allocated_rust_closure: syn::Expr = match closure_representation {
                FnRepresentation::Impl => rust_closure,
                FnRepresentation::RefDyn => syn::Expr::Call(syn::ExprCall {
                    attrs: vec![],
                    func: Box::new(syn_expr_reference(["alloc_fn_as_dyn"])),
                    paren_token: syn::token::Paren(syn_span()),
                    args: [
                        syn_expr_reference([default_allocator_parameter_name]),
                        rust_closure,
                    ]
                    .into_iter()
                    .collect(),
                }),
            };
            let full_rust: syn::Expr = if rust_clones_before_closure.is_empty() {
                maybe_allocated_rust_closure
            } else {
                rust_clones_before_closure
                    .push(syn::Stmt::Expr(maybe_allocated_rust_closure, None));
                syn::Expr::Block(syn::ExprBlock {
                    attrs: vec![],
                    label: None,
                    block: syn::Block {
                        brace_token: syn::token::Brace(syn_span()),
                        stmts: rust_clones_before_closure,
                    },
                })
            };
            CompiledStillExpression {
                uses_allocator: closure_representation == FnRepresentation::RefDyn
                    || compiled_result.uses_allocator,
                type_: input_type_maybes
                    .into_iter()
                    .collect::<Option<Vec<_>>>()
                    .zip(compiled_result.type_)
                    .map(|(input_types, result_type)| StillType::Function {
                        inputs: input_types,
                        output: Box::new(result_type),
                    }),
                rust: full_rust,
            }
        }
        StillSyntaxExpression::Let {
            declaration: maybe_declaration,
            result: maybe_result,
        } => match maybe_declaration {
            None => maybe_still_syntax_expression_to_rust(
                errors,
                || StillErrorNode {
                    range: expression_node.range,
                    message: Box::from(
                        "missing result expression after let declaration let ... here",
                    ),
                },
                records_used,
                type_aliases,
                choice_types,
                project_variable_declarations,
                local_bindings,
                closure_representation,
                maybe_result.as_ref().map(still_syntax_node_unbox),
            ),
            Some(declaration_node) => still_syntax_let_declaration_to_rust_into(
                errors,
                records_used,
                type_aliases,
                choice_types,
                project_variable_declarations,
                local_bindings,
                closure_representation,
                still_syntax_node_as_ref(declaration_node),
                maybe_result.as_ref().map(still_syntax_node_unbox),
            ),
        },
        StillSyntaxExpression::Vec(elements) => {
            if elements.is_empty() {
                errors.push(StillErrorNode {
                    range: expression_node.range,
                    message: Box::from("an empty vec needs a type :here:[]"),
                });
            }
            let mut uses_allocator: bool = false;
            let (rust_elements, element_type_maybes): (
                syn::punctuated::Punctuated<syn::Expr, syn::token::Comma>,
                Vec<Option<StillType>>,
            ) = elements
                .iter()
                .map(|element_node| {
                    let compiled: CompiledStillExpression = still_syntax_expression_to_rust(
                        errors,
                        records_used,
                        type_aliases,
                        choice_types,
                        project_variable_declarations,
                        local_bindings.clone(),
                        FnRepresentation::RefDyn,
                        still_syntax_node_as_ref(element_node),
                    );
                    uses_allocator = uses_allocator || compiled.uses_allocator;
                    (compiled.rust, compiled.type_)
                })
                .unzip();
            CompiledStillExpression {
                uses_allocator: uses_allocator,
                type_: element_type_maybes
                    .into_iter()
                    .collect::<Option<Vec<StillType>>>()
                    .and_then(|element_types| {
                        // TODO verify all elements are equal
                        let element_type: StillType = element_types.into_iter().next()?;
                        Some(still_type_vec(element_type))
                    }),
                rust: syn::Expr::Call(syn::ExprCall {
                    attrs: vec![],
                    func: Box::new(syn_expr_reference(["vec_literal"])),
                    paren_token: syn::token::Paren(syn_span()),
                    args: std::iter::once(syn::Expr::Array(syn::ExprArray {
                        attrs: vec![],
                        bracket_token: syn::token::Bracket(syn_span()),
                        elems: rust_elements,
                    }))
                    .collect(),
                }),
            }
        }
        StillSyntaxExpression::Parenthesized(maybe_in_parens) => {
            maybe_still_syntax_expression_to_rust(
                errors,
                || StillErrorNode {
                    range: expression_node.range,
                    message: Box::from("missing expression in parens between (here)"),
                },
                records_used,
                type_aliases,
                choice_types,
                project_variable_declarations,
                local_bindings.clone(),
                closure_representation,
                maybe_in_parens.as_ref().map(still_syntax_node_unbox),
            )
        }
        StillSyntaxExpression::WithComment {
            comment: comment_node,
            expression: maybe_after_comment,
        } => match maybe_after_comment {
            None => {
                errors.push(StillErrorNode {
                    range: expression_node.range,
                    message: Box::from(
                        "missing expression after linebreak after comment # ...\\n here",
                    ),
                });
                CompiledStillExpression {
                    uses_allocator: false,
                    type_: None,
                    rust: syn::Expr::Macro(syn::ExprMacro {
                        attrs: vec![],
                        mac: syn::Macro {
                            path: syn_path_reference(["std", "todo"]),
                            bang_token: syn::token::Not(syn_span()),
                            delimiter: syn::MacroDelimiter::Paren(syn::token::Paren(syn_span())),
                            tokens: proc_macro2::TokenStream::from(
                                proc_macro2::TokenTree::Literal(proc_macro2::Literal::string(
                                    &comment_node.value,
                                )),
                            ),
                        },
                    }),
                }
            }
            Some(after_comment_node) => {
                let compiled_after_comment: CompiledStillExpression =
                    still_syntax_expression_to_rust(
                        errors,
                        records_used,
                        type_aliases,
                        choice_types,
                        project_variable_declarations,
                        local_bindings.clone(),
                        closure_representation,
                        still_syntax_node_unbox(after_comment_node),
                    );
                CompiledStillExpression {
                    uses_allocator: compiled_after_comment.uses_allocator,
                    type_: compiled_after_comment.type_,
                    rust: syn::Expr::Paren(syn::ExprParen {
                        attrs: vec![syn_attribute_doc(&comment_node.value)],

                        paren_token: syn::token::Paren(syn_span()),
                        expr: Box::new(compiled_after_comment.rust),
                    }),
                }
            }
        },
        StillSyntaxExpression::Typed {
            type_: maybe_type_node,
            expression: maybe_in_typed,
        } => {
            let maybe_expected_type: Option<StillType> = match maybe_type_node {
                Some(type_node) => still_syntax_type_to_type(
                    errors,
                    type_aliases,
                    choice_types,
                    still_syntax_node_as_ref(type_node),
                ),
                None => {
                    errors.push(StillErrorNode {
                        range: expression_node.range,
                        message: Box::from("missing type between colons :here:..expression.."),
                    });
                    None
                }
            };
            match maybe_in_typed {
                None => {
                    errors.push(StillErrorNode {
                        range: expression_node.range,
                        message: Box::from("missing expression after type :...: here"),
                    });
                    CompiledStillExpression {
                        uses_allocator: false,
                        type_: maybe_expected_type,
                        rust: syn_expr_todo(),
                    }
                }
                Some(untyped_node) => match &untyped_node.value {
                    StillSyntaxExpressionUntyped::Variant {
                        name: name_node,
                        value: maybe_value,
                    } => {
                        let Some(type_) = maybe_expected_type else {
                            return CompiledStillExpression {
                                uses_allocator: false,
                                rust: syn_expr_todo(),
                                type_: None,
                            };
                        };
                        let StillType::ChoiceConstruct {
                            name: origin_choice_type_name,
                            arguments: origin_choice_type_arguments,
                        } = type_
                        else {
                            errors.push(StillErrorNode {
                                range: maybe_type_node.as_ref().map(|n| n.range).unwrap_or(expression_node.range),
                                message: Box::from("type in :here: is not a choice type which is necessary for a variant")
                            });
                            return CompiledStillExpression {
                                uses_allocator: false,
                                rust: syn_expr_todo(),
                                type_: None,
                            };
                        };
                        let variant_value_needs_to_be_reference: bool = 'variant_value_is_reference: {
                            let Some(origin_choice_type) =
                                choice_types.get(origin_choice_type_name.as_str())
                            else {
                                break 'variant_value_is_reference false;
                            };
                            let Some(variant_index_in_origin_choice_type) = origin_choice_type
                                .variants
                                .iter()
                                .enumerate()
                                .find(|(_, origin_choice_type_variant)| {
                                    origin_choice_type_variant.name.as_ref().is_some_and(
                                        |origin_choice_type_variant_name_node| {
                                            origin_choice_type_variant_name_node.value
                                                == name_node.value
                                        },
                                    )
                                })
                                .map(|(i, _)| i)
                            else {
                                break 'variant_value_is_reference false;
                            };
                            origin_choice_type
                                .type_variants
                                .get(variant_index_in_origin_choice_type)
                                .and_then(|type_variant| type_variant.value.as_ref())
                                .is_some_and(|variant_value| {
                                    variant_value.constructs_recursive_type
                                })
                        };
                        let rust_variant_reference: syn::Expr = syn_expr_reference([
                            &still_name_to_uppercase_rust(&origin_choice_type_name),
                            &still_name_to_uppercase_rust(&name_node.value),
                        ]);
                        match maybe_value {
                            None => {
                                // TODO check origin variant also has no value
                                CompiledStillExpression {
                                    uses_allocator: false,
                                    type_: Some(StillType::ChoiceConstruct {
                                        name: origin_choice_type_name,
                                        arguments: origin_choice_type_arguments,
                                    }),
                                    rust: rust_variant_reference,
                                }
                            }
                            Some(value_node) => {
                                let value_compiled: CompiledStillExpression =
                                    still_syntax_expression_to_rust(
                                        errors,
                                        records_used,
                                        type_aliases,
                                        choice_types,
                                        project_variable_declarations,
                                        local_bindings,
                                        FnRepresentation::RefDyn,
                                        still_syntax_node_unbox(value_node),
                                    );
                                // TODO verify equal: origin choice type variant value with the type arguments inlined & value pattern type
                                CompiledStillExpression {
                                    uses_allocator: variant_value_needs_to_be_reference
                                        || value_compiled.uses_allocator,
                                    type_: Some(StillType::ChoiceConstruct {
                                        name: origin_choice_type_name,
                                        arguments: origin_choice_type_arguments,
                                    }),
                                    rust: syn::Expr::Call(syn::ExprCall {
                                        attrs: vec![],
                                        func: Box::new(rust_variant_reference),
                                        paren_token: syn::token::Paren(syn_span()),
                                        args: std::iter::once({
                                            if variant_value_needs_to_be_reference {
                                                syn_expr_call_alloc_method(value_compiled.rust)
                                            } else {
                                                value_compiled.rust
                                            }
                                        })
                                        .collect(),
                                    }),
                                }
                            }
                        }
                    }
                    StillSyntaxExpressionUntyped::Other(other_expression) => {
                        if let StillSyntaxExpression::Vec(elements) = other_expression.as_ref()
                            && elements.is_empty()
                        {
                            CompiledStillExpression {
                                rust: syn::Expr::Call(syn::ExprCall {
                                    attrs: vec![],
                                    func: Box::new(syn_expr_reference(["vec_literal"])),
                                    paren_token: syn::token::Paren(syn_span()),
                                    args: std::iter::once(syn::Expr::Array(syn::ExprArray {
                                        attrs: vec![],
                                        bracket_token: syn::token::Bracket(syn_span()),
                                        elems: syn::punctuated::Punctuated::new(),
                                    }))
                                    .collect(),
                                }),
                                uses_allocator: false,
                                // TODO check expected_type resolves to some vec
                                type_: maybe_expected_type,
                            }
                        } else {
                            // TODO verify equal to maybe_expected_type
                            still_syntax_expression_to_rust(
                                errors,
                                records_used,
                                type_aliases,
                                choice_types,
                                project_variable_declarations,
                                local_bindings,
                                closure_representation,
                                StillSyntaxNode {
                                    range: untyped_node.range,
                                    value: other_expression,
                                },
                            )
                        }
                    }
                },
            }
        }
        StillSyntaxExpression::VariableOrCall {
            variable: variable_node,
            arguments,
        } => {
            let rust_variable_name: String = still_name_to_lowercase_rust(&variable_node.value);
            let mut uses_allocator: bool = false;
            let (rust_arguments, argument_maybe_types): (Vec<syn::Expr>, Vec<Option<StillType>>) =
                arguments
                    .iter()
                    .map(|argument_node| {
                        let compiled_argument: CompiledStillExpression =
                            still_syntax_expression_to_rust(
                                errors,
                                records_used,
                                type_aliases,
                                choice_types,
                                project_variable_declarations,
                                local_bindings.clone(),
                                // TODO ::Impl for project variables
                                FnRepresentation::RefDyn,
                                still_syntax_node_as_ref(argument_node),
                            );
                        uses_allocator = uses_allocator || compiled_argument.uses_allocator;
                        (compiled_argument.rust, compiled_argument.type_)
                    })
                    .unzip();
            let rust_reference: syn::Expr = syn_expr_reference([&rust_variable_name]);
            match local_bindings.get(variable_node.value.as_str()) {
                Some(variable_info) => {
                    let Some(variable_type) = &variable_info.type_ else {
                        return CompiledStillExpression {
                            rust: syn_expr_todo(),
                            uses_allocator: false,
                            type_: None,
                        };
                    };
                    let type_: StillType = if arguments.is_empty() {
                        variable_type.clone()
                    } else {
                        match variable_type {
                            StillType::Function {
                                inputs: variable_input_types,
                                output: variable_output_type,
                            } => {
                                match variable_input_types.len().cmp(&arguments.len()) {
                                    std::cmp::Ordering::Equal => {}
                                    std::cmp::Ordering::Less => {
                                        errors.push(StillErrorNode {
                                            range: variable_node.range,
                                            message: format!(
                                                "too many arguments. Expected {} less. To call a function that is the result of a function, store it in an intermediate let and call that variable",
                                                arguments.len() - variable_input_types.len()
                                            ).into_boxed_str()
                                        });
                                    }
                                    std::cmp::Ordering::Greater => {
                                        errors.push(StillErrorNode {
                                            range: variable_node.range,
                                            message: format!(
                                                "missing arguments. Expected {} more. Note that partial application is not a feature in still. Instead, wrap this call in a lambda that accepts and applies the remaining arguments",
                                                variable_input_types.len() - arguments.len()
                                            ).into_boxed_str()
                                        });
                                    }
                                }
                                (**variable_output_type).clone()
                            }
                            _ => {
                                errors.push(StillErrorNode { range: variable_node.range, message: Box::from("calling a variable whose type is not a function. Maybe you forgot a separating comma or similar?") });
                                return CompiledStillExpression {
                                    rust: syn_expr_todo(),
                                    uses_allocator: false,
                                    type_: None,
                                };
                            }
                        }
                    };
                    let rust_reference_cloned_if_necessary: syn::Expr = if variable_info.is_copy
                        || variable_info.last_uses.contains(&variable_node.range)
                    {
                        rust_reference
                    } else {
                        syn_expr_call_clone_method(rust_reference)
                    };
                    CompiledStillExpression {
                        rust: if arguments.is_empty() {
                            rust_reference_cloned_if_necessary
                        } else {
                            syn::Expr::Call(syn::ExprCall {
                                attrs: vec![],
                                func: Box::new(rust_reference_cloned_if_necessary),
                                paren_token: syn::token::Paren(syn_span()),
                                args: rust_arguments.into_iter().collect(),
                            })
                        },
                        uses_allocator: uses_allocator,
                        type_: Some(type_),
                    }
                }
                None => {
                    let Some(project_variable_info) =
                        project_variable_declarations.get(variable_node.value.as_str())
                    else {
                        errors.push(StillErrorNode { range: variable_node.range, message: Box::from("unknown variable. No project variable or local variable has this name. Check for typos.") });
                        return CompiledStillExpression {
                            rust: syn_expr_todo(),
                            uses_allocator: false,
                            type_: None,
                        };
                    };
                    let Some(project_variable_type) = &project_variable_info.type_ else {
                        errors.push(StillErrorNode { range: variable_node.range, message: Box::from("this project variable has an incomplete type. Go to that variable's declaration and fix its errors. If there aren't any, these declarations are (mutually) recursive and need an explicit output type! You can add one by prepending :type: before any expression like the result of a lambda.") });
                        return CompiledStillExpression {
                            rust: syn_expr_todo(),
                            uses_allocator: false,
                            type_: None,
                        };
                    };
                    match project_variable_info.kind {
                        RustVariableItemKind::Fn => {}
                        RustVariableItemKind::Static => {
                            return CompiledStillExpression {
                                rust: rust_reference,
                                uses_allocator: false,
                                type_: Some(project_variable_type.clone()),
                            };
                        }
                    }
                    let type_: StillType = if arguments.is_empty() {
                        project_variable_type.clone()
                    } else {
                        match project_variable_type {
                            StillType::Function {
                                inputs: project_variable_input_types,
                                output: project_variable_output_type,
                            } => {
                                // optimization possibility: when output contains no type variables,
                                // just return it
                                match project_variable_input_types.len().cmp(&arguments.len()) {
                                    std::cmp::Ordering::Equal => {}
                                    std::cmp::Ordering::Less => {
                                        errors.push(StillErrorNode {
                                            range: variable_node.range,
                                            message: format!(
                                                "too many arguments. Expected {} less. To call a function that is the result of a function, store it in an intermediate let and call that variable",
                                                arguments.len() - project_variable_input_types.len()
                                            ).into_boxed_str()
                                        });
                                    }
                                    std::cmp::Ordering::Greater => {
                                        errors.push(StillErrorNode {
                                            range: variable_node.range,
                                            message: format!(
                                                "missing arguments. Expected {} more. Note that partial application is not a feature in still. Instead, wrap this call in a lambda that accepts and applies the remaining arguments",
                                                project_variable_input_types.len() - arguments.len()
                                            ).into_boxed_str()
                                        });
                                    }
                                }
                                let mut type_parameter_replacements: std::collections::HashMap<
                                    Box<str>,
                                    StillType,
                                > = std::collections::HashMap::new();
                                for (parameter_type_node, maybe_argument_type) in
                                    project_variable_input_types
                                        .iter()
                                        .zip(argument_maybe_types.into_iter())
                                {
                                    if let Some(argument_type) = maybe_argument_type {
                                        still_type_collect_variables_that_are_concrete_into(
                                            &mut type_parameter_replacements,
                                            parameter_type_node,
                                            argument_type,
                                        );
                                    }
                                }
                                let mut variable_output_type: StillType =
                                    (**project_variable_output_type).clone();
                                still_type_replace_variables(
                                    // seems inefficient, a function would be better
                                    &type_parameter_replacements
                                        .iter()
                                        .map(|(k, v)| (k.as_ref(), v))
                                        .collect::<std::collections::HashMap<_, _>>(),
                                    &mut variable_output_type,
                                );
                                variable_output_type
                            }
                            _ => {
                                errors.push(StillErrorNode { range: variable_node.range, message: Box::from("calling a variable whose type is not a function. Maybe you forgot a separating comma or similar?") });
                                return CompiledStillExpression {
                                    rust: syn_expr_todo(),
                                    uses_allocator: false,
                                    type_: None,
                                };
                            }
                        }
                    };
                    CompiledStillExpression {
                        rust: syn::Expr::Call(syn::ExprCall {
                            attrs: vec![],
                            func: Box::new(rust_reference),
                            paren_token: syn::token::Paren(syn_span()),
                            args: if project_variable_info.has_allocator_parameter {
                                Some(syn_expr_reference([default_allocator_parameter_name]))
                            } else {
                                None
                            }
                            .into_iter()
                            .chain(rust_arguments)
                            .collect(),
                        }),
                        uses_allocator: project_variable_info.has_allocator_parameter
                            || uses_allocator,
                        type_: Some(type_),
                    }
                }
            }
        }
        StillSyntaxExpression::Match {
            matched: matched_node,
            cases,
        } => {
            let compiled_matched: CompiledStillExpression = still_syntax_expression_to_rust(
                errors,
                records_used,
                type_aliases,
                choice_types,
                project_variable_declarations,
                local_bindings.clone(),
                FnRepresentation::RefDyn,
                still_syntax_node_unbox(matched_node),
            );
            let mut arms_use_allocator: bool = false;
            let (mut rust_arms, case_result_types): (Vec<syn::Arm>, Vec<Option<StillType>>) = cases
                .iter()
                .filter_map(|case| {
                    let Some(case_pattern_node) = &case.pattern else {
                        errors.push(StillErrorNode {
                            range: case.or_bar_key_symbol_range,
                            message: Box::from("missing case pattern in | here > ..result.."),
                        });
                        return None;
                    };
                    let mut case_pattern_introduced_bindings: std::collections::HashMap<
                        &str,
                        StillLocalBindingCompileInfo,
                    > = std::collections::HashMap::new();
                    let mut bindings_to_clone: Vec<BindingToClone> = Vec::new();
                    let compiled_pattern: CompiledStillPattern = still_syntax_pattern_to_rust(
                        errors,
                        records_used,
                        &mut case_pattern_introduced_bindings,
                        &mut bindings_to_clone,
                        type_aliases,
                        choice_types,
                        false,
                        still_syntax_node_as_ref(case_pattern_node),
                    );
                    for (parameter_introduced_binding_name, parameter_introduced_binding_info) in
                        &case_pattern_introduced_bindings
                    {
                        push_error_if_name_collides(
                            errors,
                            project_variable_declarations,
                            &local_bindings,
                            StillSyntaxNode {
                                range: parameter_introduced_binding_info.origin_range,
                                value: parameter_introduced_binding_name,
                            },
                        );
                    }
                    let Some(rust_pattern) = compiled_pattern.rust else {
                        // skip case with incomplete pattern
                        return None;
                    };
                    if let Some(case_result_node) = &case.result {
                        still_syntax_expression_uses_of_local_bindings_into(
                            &mut case_pattern_introduced_bindings,
                            None,
                            still_syntax_node_as_ref(case_result_node),
                        );
                    }
                    let mut local_bindings: std::collections::HashMap<
                        &str,
                        StillLocalBindingCompileInfo,
                    > = (*local_bindings).clone();
                    local_bindings.extend(case_pattern_introduced_bindings);
                    let compiled_case_result: CompiledStillExpression =
                        maybe_still_syntax_expression_to_rust(
                            errors,
                            || StillErrorNode {
                                range: case
                                    .arrow_key_symbol_range
                                    .unwrap_or(case_pattern_node.range),
                                message: Box::from(
                                    "missing case result after | ..pattern.. > here",
                                ),
                            },
                            records_used,
                            type_aliases,
                            choice_types,
                            project_variable_declarations,
                            std::rc::Rc::new(local_bindings),
                            FnRepresentation::RefDyn,
                            case.result.as_ref().map(still_syntax_node_as_ref),
                        );
                    // TODO check all pattern types are equal to matched type
                    arms_use_allocator = arms_use_allocator || compiled_case_result.uses_allocator;
                    let mut rust_stmts: Vec<syn::Stmt> = Vec::with_capacity(1);
                    bindings_to_clone_to_rust_into(&mut rust_stmts, bindings_to_clone);
                    rust_stmts.push(syn::Stmt::Expr(compiled_case_result.rust, None));
                    Some((
                        syn::Arm {
                            attrs: vec![],
                            pat: rust_pattern,
                            guard: None,
                            fat_arrow_token: syn::token::FatArrow(syn_span()),
                            body: Box::new(syn::Expr::Block(syn::ExprBlock {
                                attrs: vec![],
                                label: None,
                                block: syn::Block {
                                    brace_token: syn::token::Brace(syn_span()),
                                    stmts: rust_stmts,
                                },
                            })),
                            comma: None,
                        },
                        compiled_case_result.type_,
                    ))
                })
                .unzip();
            // _ => todo!() is appended to still make inexhaustive matching compile
            // and be able to be run, rust will emit a warning
            // TODO remove when can be determined to be exhaustive,
            // otherwise also add error
            rust_arms.push(syn::Arm {
                attrs: vec![],
                pat: syn::Pat::Wild(syn::PatWild {
                    attrs: vec![],
                    underscore_token: syn::token::Underscore(syn_span()),
                }),
                fat_arrow_token: syn::token::FatArrow(syn_span()),
                guard: None,
                body: Box::new(syn_expr_todo()),
                comma: None,
            });
            CompiledStillExpression {
                rust: syn::Expr::Match(syn::ExprMatch {
                    attrs: vec![],
                    match_token: syn::token::Match(syn_span()),
                    expr: Box::new(compiled_matched.rust),
                    brace_token: syn::token::Brace(syn_span()),
                    arms: rust_arms,
                }),
                type_: case_result_types
                    .into_iter()
                    // TODO check all result types are equal
                    .find_map(|case_result_type| case_result_type),
                uses_allocator: compiled_matched.uses_allocator || arms_use_allocator,
            }
        }
        StillSyntaxExpression::Record(fields) => {
            records_used.insert(sorted_field_names(
                fields.iter().map(|field| &field.name.value),
            ));
            let mut fields_use_allocator: bool = false;
            let (rust_fields, field_maybe_types): (
                syn::punctuated::Punctuated<syn::FieldValue, syn::token::Comma>,
                Vec<Option<StillTypeField>>,
            ) = fields
                .iter()
                .map(|field| {
                    let compiled_field_value: CompiledStillExpression =
                        maybe_still_syntax_expression_to_rust(
                            errors,
                            || StillErrorNode {
                                range: field.name.range,
                                message: Box::from(
                                    "missing field value expression after this field name",
                                ),
                            },
                            records_used,
                            type_aliases,
                            choice_types,
                            project_variable_declarations,
                            local_bindings.clone(),
                            closure_representation,
                            field.value.as_ref().map(still_syntax_node_as_ref),
                        );
                    fields_use_allocator =
                        fields_use_allocator || compiled_field_value.uses_allocator;
                    (
                        syn::FieldValue {
                            attrs: vec![],
                            member: syn::Member::Named(syn_ident(&still_name_to_lowercase_rust(
                                &field.name.value,
                            ))),
                            colon_token: Some(syn::token::Colon(syn_span())),
                            expr: compiled_field_value.rust,
                        },
                        compiled_field_value.type_.map(|value_type| StillTypeField {
                            name: field.name.value.clone(),
                            value: value_type,
                        }),
                    )
                })
                .unzip();
            CompiledStillExpression {
                rust: syn::Expr::Struct(syn::ExprStruct {
                    attrs: vec![],
                    qself: None,
                    path: syn_path_reference([&still_field_names_to_rust_record_struct_name(
                        fields.iter().map(|field| field.name.value.as_ref()),
                    )]),
                    brace_token: syn::token::Brace(syn_span()),
                    fields: rust_fields,
                    dot2_token: None,
                    rest: None,
                }),
                uses_allocator: fields_use_allocator,
                type_: field_maybe_types
                    .into_iter()
                    .collect::<Option<Vec<StillTypeField>>>()
                    .map(StillType::Record),
            }
        }
        StillSyntaxExpression::RecordAccess {
            record: record_node,
            field: maybe_field_name,
        } => {
            let compiled_record: CompiledStillExpression = still_syntax_expression_to_rust(
                errors,
                records_used,
                type_aliases,
                choice_types,
                project_variable_declarations,
                local_bindings,
                FnRepresentation::RefDyn,
                still_syntax_node_unbox(record_node),
            );
            let Some(field_name_node) = maybe_field_name else {
                errors.push(StillErrorNode { range: expression_node.range, message: Box::from("missing field name in record access (..record..).here. Field names start with a lowercase letter a-z") });
                return compiled_record;
            };
            let Some(record_type) = compiled_record.type_ else {
                return CompiledStillExpression {
                    rust: syn_expr_todo(),
                    uses_allocator: false,
                    type_: None,
                };
            };
            let StillType::Record(record_type_fields) = record_type else {
                errors.push(StillErrorNode {
                    range: field_name_node.range,
                    message: Box::from(
                        "cannot access record field on expression whose type is not a record",
                    ),
                });
                return CompiledStillExpression {
                    rust: syn_expr_todo(),
                    uses_allocator: false,
                    type_: None,
                };
            };
            let Some(accessed_record_type_field) = record_type_fields
                .iter()
                .find(|record_type_field| record_type_field.name == field_name_node.value)
            else {
                errors.push(StillErrorNode {
                    range: field_name_node.range,
                    message: format!(
                        "cannot access record field on expression whose type is a record without that field. Available fields are {}",
                        record_type_fields.iter().map(|field| field.name.as_str()).collect::<Vec<&str>>().join(", "),
                    ).into_boxed_str(),
                });
                return CompiledStillExpression {
                    rust: syn_expr_todo(),
                    uses_allocator: false,
                    type_: None,
                };
            };
            CompiledStillExpression {
                uses_allocator: compiled_record.uses_allocator,
                rust: syn::Expr::Field(syn::ExprField {
                    attrs: vec![],
                    base: Box::new(compiled_record.rust),
                    dot_token: syn::token::Dot(syn_span()),
                    member: syn::Member::Named(syn_ident(&still_name_to_lowercase_rust(
                        &field_name_node.value,
                    ))),
                }),
                type_: Some(accessed_record_type_field.value.clone()),
            }
        }
        StillSyntaxExpression::RecordUpdate {
            record: maybe_record,
            spread_key_symbol_range: _,
            fields,
        } => match maybe_record {
            None => {
                errors.push(StillErrorNode {
                    range: expression_node.range,
                    message: Box::from(
                        "missing record expression to update in { ..here, ... ... }",
                    ),
                });
                CompiledStillExpression {
                    uses_allocator: false,
                    rust: syn_expr_todo(),
                    type_: None,
                }
            }
            Some(record_node) => {
                let mut fields_use_allocator: bool = false;
                let rust_fields = fields
                    .iter()
                    .map(|field| {
                        let compiled_field_value: CompiledStillExpression =
                            maybe_still_syntax_expression_to_rust(
                                errors,
                                || StillErrorNode {
                                    range: field.name.range,
                                    message: Box::from("missing field value after this field name"),
                                },
                                records_used,
                                type_aliases,
                                choice_types,
                                project_variable_declarations,
                                local_bindings.clone(),
                                closure_representation,
                                field.value.as_ref().map(still_syntax_node_as_ref),
                            );
                        fields_use_allocator =
                            fields_use_allocator || compiled_field_value.uses_allocator;
                        syn::FieldValue {
                            attrs: vec![],
                            member: syn::Member::Named(syn_ident(&still_name_to_lowercase_rust(
                                &field.name.value,
                            ))),
                            colon_token: Some(syn::token::Colon(syn_span())),
                            expr: compiled_field_value.rust,
                        }
                    })
                    .collect();
                let compiled_record: CompiledStillExpression = still_syntax_expression_to_rust(
                    errors,
                    records_used,
                    type_aliases,
                    choice_types,
                    project_variable_declarations,
                    local_bindings,
                    FnRepresentation::RefDyn,
                    still_syntax_node_unbox(record_node),
                );
                // TODO check the updated field values are present in the compile record type
                // and their value types are equal
                CompiledStillExpression {
                    rust: syn::Expr::Struct(syn::ExprStruct {
                        attrs: vec![],
                        qself: None,
                        path: syn_path_reference([&still_field_names_to_rust_record_struct_name(
                            fields.iter().map(|field| field.name.value.as_ref()),
                        )]),
                        brace_token: syn::token::Brace(syn_span()),
                        fields: rust_fields,
                        dot2_token: Some(syn::token::DotDot(syn_span())),
                        rest: Some(Box::new(compiled_record.rust)),
                    }),
                    uses_allocator: compiled_record.uses_allocator || fields_use_allocator,
                    type_: compiled_record.type_,
                }
            }
        },
    }
}
/// If called from outside itself, set `in_closures` to `None`
fn still_syntax_expression_uses_of_local_bindings_into<'a>(
    local_binding_infos: &mut std::collections::HashMap<&'a str, StillLocalBindingCompileInfo>,
    maybe_in_closure: Option<lsp_types::Range>,
    expression_node: StillSyntaxNode<&'a StillSyntaxExpression>,
) {
    match expression_node.value {
        StillSyntaxExpression::Char(_) => {}
        StillSyntaxExpression::Dec(_) => {}
        StillSyntaxExpression::Int(_) => {}
        StillSyntaxExpression::String { .. } => {}
        StillSyntaxExpression::Parenthesized(maybe_in_parens) => {
            if let Some(in_parens_node) = maybe_in_parens {
                still_syntax_expression_uses_of_local_bindings_into(
                    local_binding_infos,
                    maybe_in_closure,
                    still_syntax_node_unbox(in_parens_node),
                );
            }
        }
        StillSyntaxExpression::WithComment {
            comment: _,
            expression: maybe_after_comment,
        } => {
            if let Some(after_comment_node) = maybe_after_comment {
                still_syntax_expression_uses_of_local_bindings_into(
                    local_binding_infos,
                    maybe_in_closure,
                    still_syntax_node_unbox(after_comment_node),
                );
            }
        }
        StillSyntaxExpression::Typed {
            type_: _,
            expression: maybe_untyped,
        } => {
            if let Some(untyped_node) = maybe_untyped {
                match &untyped_node.value {
                    StillSyntaxExpressionUntyped::Variant {
                        name: _,
                        value: maybe_value,
                    } => {
                        if let Some(value_node) = maybe_value {
                            still_syntax_expression_uses_of_local_bindings_into(
                                local_binding_infos,
                                maybe_in_closure,
                                still_syntax_node_unbox(value_node),
                            );
                        }
                    }
                    StillSyntaxExpressionUntyped::Other(other_node) => {
                        still_syntax_expression_uses_of_local_bindings_into(
                            local_binding_infos,
                            maybe_in_closure,
                            StillSyntaxNode {
                                range: untyped_node.range,
                                value: other_node,
                            },
                        );
                    }
                }
            }
        }
        StillSyntaxExpression::VariableOrCall {
            variable: variable_node,
            arguments,
        } => {
            if let Some(local_binding_info) =
                local_binding_infos.get_mut(variable_node.value.as_str())
            {
                local_binding_info.last_uses.clear();
                match maybe_in_closure {
                    None => {
                        local_binding_info.last_uses.push(variable_node.range);
                    }
                    Some(in_closure) => {
                        local_binding_info.closures_it_is_used_in.push(in_closure);
                        // the variables in closures are considered their own thing
                        // since they e.g. always need to be cloned
                        local_binding_info.last_uses.push(in_closure);
                    }
                }
            }
            for argument_node in arguments {
                still_syntax_expression_uses_of_local_bindings_into(
                    local_binding_infos,
                    maybe_in_closure,
                    still_syntax_node_as_ref(argument_node),
                );
            }
        }
        StillSyntaxExpression::Match {
            matched: matched_node,
            cases,
        } => {
            still_syntax_expression_uses_of_local_bindings_into(
                local_binding_infos,
                maybe_in_closure,
                still_syntax_node_unbox(matched_node),
            );
            if let Some((last_case, cases_before_last)) = cases.split_last() {
                if let Some(last_case_result) = &last_case.result {
                    still_syntax_expression_uses_of_local_bindings_into(
                        local_binding_infos,
                        maybe_in_closure,
                        still_syntax_node_as_ref(last_case_result),
                    );
                }
                // we collect last uses separately for each case because
                // cases are not run in sequence but exclusively one of them
                for case_result in cases_before_last
                    .iter()
                    .filter_map(|case| case.result.as_ref())
                {
                    let mut local_bindings_last_uses_in_branch: std::collections::HashMap<
                        &str,
                        StillLocalBindingCompileInfo,
                    > = std::collections::HashMap::new();
                    still_syntax_expression_uses_of_local_bindings_into(
                        &mut local_bindings_last_uses_in_branch,
                        maybe_in_closure,
                        still_syntax_node_as_ref(case_result),
                    );
                    for (local_binding_name, local_binding_info_in_branch) in
                        local_bindings_last_uses_in_branch
                    {
                        match local_binding_infos.get_mut(local_binding_name) {
                            None => {
                                local_binding_infos
                                    .insert(local_binding_name, local_binding_info_in_branch);
                            }
                            Some(existing) => {
                                existing
                                    .last_uses
                                    .extend(local_binding_info_in_branch.last_uses);
                                existing
                                    .closures_it_is_used_in
                                    .extend(local_binding_info_in_branch.closures_it_is_used_in);
                            }
                        }
                    }
                }
            }
        }
        StillSyntaxExpression::Lambda {
            parameters: _,
            arrow_key_symbol_range: _,
            result: maybe_result,
        } => {
            if let Some(result_node) = maybe_result {
                still_syntax_expression_uses_of_local_bindings_into(
                    local_binding_infos,
                    Some(maybe_in_closure.unwrap_or(expression_node.range)),
                    still_syntax_node_unbox(result_node),
                );
            }
        }
        StillSyntaxExpression::Let {
            declaration: maybe_declaration,
            result: maybe_result,
        } => {
            if let Some(declaration_node) = maybe_declaration
                && let Some(declaration_result_node) = &declaration_node.value.result
            {
                still_syntax_expression_uses_of_local_bindings_into(
                    local_binding_infos,
                    maybe_in_closure,
                    still_syntax_node_unbox(declaration_result_node),
                );
            }
            if let Some(result_node) = maybe_result {
                still_syntax_expression_uses_of_local_bindings_into(
                    local_binding_infos,
                    maybe_in_closure,
                    still_syntax_node_unbox(result_node),
                );
            }
        }
        StillSyntaxExpression::Vec(elements) => {
            for element_node in elements {
                still_syntax_expression_uses_of_local_bindings_into(
                    local_binding_infos,
                    maybe_in_closure,
                    still_syntax_node_as_ref(element_node),
                );
            }
        }
        StillSyntaxExpression::Record(fields) => {
            for field_vale_node in fields.iter().filter_map(|field| field.value.as_ref()) {
                still_syntax_expression_uses_of_local_bindings_into(
                    local_binding_infos,
                    maybe_in_closure,
                    still_syntax_node_as_ref(field_vale_node),
                );
            }
        }
        StillSyntaxExpression::RecordAccess {
            record: record_node,
            field: _,
        } => {
            still_syntax_expression_uses_of_local_bindings_into(
                local_binding_infos,
                maybe_in_closure,
                still_syntax_node_unbox(record_node),
            );
        }
        StillSyntaxExpression::RecordUpdate {
            record: maybe_record,
            spread_key_symbol_range: _,
            fields,
        } => {
            for field_vale_node in fields.iter().filter_map(|field| field.value.as_ref()) {
                still_syntax_expression_uses_of_local_bindings_into(
                    local_binding_infos,
                    maybe_in_closure,
                    still_syntax_node_as_ref(field_vale_node),
                );
            }
            // because in rust the record to update comes after the fields
            if let Some(record_node) = maybe_record {
                still_syntax_expression_uses_of_local_bindings_into(
                    local_binding_infos,
                    maybe_in_closure,
                    still_syntax_node_unbox(record_node),
                );
            }
        }
    }
}
fn push_error_if_name_collides(
    errors: &mut Vec<StillErrorNode>,
    project_variable_declarations: &std::collections::HashMap<
        StillName,
        CompiledVariableDeclarationInfo,
    >,
    local_bindings: &std::rc::Rc<std::collections::HashMap<&str, StillLocalBindingCompileInfo>>,
    name_node: StillSyntaxNode<&str>,
) {
    if project_variable_declarations.contains_key(name_node.value) {
        if core_choice_type_infos.contains_key(name_node.value) {
            errors.push(StillErrorNode {
                range: name_node.range,
                message: Box::from("a variable with this name is already part of core (core variables are for example int-to-str or dec-add). Rename this variable")
            });
        } else {
            errors.push(StillErrorNode {
                range: name_node.range,
                message: Box::from(
                    "a variable with this name is already declared in this project. Rename one of them",
                ),
            });
        }
    } else if local_bindings.contains_key(name_node.value) {
        errors.push(StillErrorNode {
            range: name_node.range,
            message: Box::from(
                "a variable with this name is already declared locally. Rename one of them",
            ),
        });
    }
}
fn still_syntax_let_declaration_to_rust_into(
    errors: &mut Vec<StillErrorNode>,
    records_used: &mut std::collections::HashSet<Vec<StillName>>,
    type_aliases: &std::collections::HashMap<StillName, TypeAliasInfo>,
    choice_types: &std::collections::HashMap<StillName, ChoiceTypeInfo>,
    project_variable_declarations: &std::collections::HashMap<
        StillName,
        CompiledVariableDeclarationInfo,
    >,
    local_bindings: std::rc::Rc<std::collections::HashMap<&str, StillLocalBindingCompileInfo>>,
    closure_representation: FnRepresentation,
    declaration_node: StillSyntaxNode<&StillSyntaxLetDeclaration>,
    maybe_result: Option<StillSyntaxNode<&StillSyntaxExpression>>,
) -> CompiledStillExpression {
    push_error_if_name_collides(
        errors,
        project_variable_declarations,
        &local_bindings,
        still_syntax_node_as_ref_map(&declaration_node.value.name, StillName::as_str),
    );
    let compiled_declaration_result: CompiledStillExpression =
        maybe_still_syntax_expression_to_rust(
            errors,
            || StillErrorNode {
                range: declaration_node.range,
                message: Box::from(
                    "missing assigned let variable declaration expression in let ..name.. here",
                ),
            },
            records_used,
            type_aliases,
            choice_types,
            project_variable_declarations,
            local_bindings.clone(),
            // could be ::Impl when all uses are allocated if necessary,
            // too much analysis with little gain I think
            FnRepresentation::RefDyn,
            declaration_node
                .value
                .result
                .as_ref()
                .map(still_syntax_node_unbox),
        );
    let mut rust_stmts: Vec<syn::Stmt> = Vec::new();
    rust_stmts.push(syn::Stmt::Local(syn::Local {
        attrs: vec![],
        let_token: syn::token::Let(syn_span()),
        pat: syn_pat_variable(&declaration_node.value.name.value),
        init: Some(syn::LocalInit {
            eq_token: syn::token::Eq(syn_span()),
            expr: Box::new(compiled_declaration_result.rust),
            diverge: None,
        }),
        semi_token: syn::token::Semi(syn_span()),
    }));
    let mut introduced_binding_infos: std::collections::HashMap<
        &str,
        StillLocalBindingCompileInfo,
    > = std::iter::once((
        declaration_node.value.name.value.as_str(),
        StillLocalBindingCompileInfo {
            origin_range: declaration_node.value.name.range,
            is_copy: compiled_declaration_result
                .type_
                .as_ref()
                .is_some_and(|result_type| {
                    still_type_is_copy(false, type_aliases, choice_types, result_type)
                }),
            type_: compiled_declaration_result.type_,
            last_uses: vec![],
            closures_it_is_used_in: vec![],
        },
    ))
    .collect::<std::collections::HashMap<_, _>>();
    let mut local_bindings: std::collections::HashMap<&str, StillLocalBindingCompileInfo> =
        std::rc::Rc::unwrap_or_clone(local_bindings);
    if let Some(result_node) = maybe_result {
        still_syntax_expression_uses_of_local_bindings_into(
            &mut introduced_binding_infos,
            None,
            result_node,
        );
    }
    local_bindings.extend(introduced_binding_infos);
    let maybe_result_compiled: CompiledStillExpression = maybe_still_syntax_expression_to_rust(
        errors,
        || StillErrorNode {
            range: declaration_node.value.name.range,
            message: Box::from("missing result expression after let declaration let ... here"),
        },
        records_used,
        type_aliases,
        choice_types,
        project_variable_declarations,
        std::rc::Rc::new(local_bindings),
        closure_representation,
        maybe_result,
    );
    CompiledStillExpression {
        uses_allocator: compiled_declaration_result.uses_allocator
            || maybe_result_compiled.uses_allocator,
        type_: maybe_result_compiled.type_,
        rust: match maybe_result_compiled.rust {
            syn::Expr::Block(rust_let_result_block) => {
                rust_stmts.extend(rust_let_result_block.block.stmts);
                syn::Expr::Block(syn::ExprBlock {
                    label: rust_let_result_block.label,
                    attrs: rust_let_result_block.attrs,
                    block: syn::Block {
                        brace_token: syn::token::Brace(syn_span()),
                        stmts: rust_stmts,
                    },
                })
            }
            _ => {
                rust_stmts.push(syn::Stmt::Expr(maybe_result_compiled.rust, None));
                syn::Expr::Block(syn::ExprBlock {
                    label: None,
                    attrs: vec![],
                    block: syn::Block {
                        brace_token: syn::token::Brace(syn_span()),
                        stmts: rust_stmts,
                    },
                })
            }
        },
    }
}

fn maybe_still_syntax_pattern_to_rust<'a>(
    errors: &mut Vec<StillErrorNode>,
    error_on_none: impl FnOnce() -> StillErrorNode,
    records_used: &mut std::collections::HashSet<Vec<StillName>>,
    introduced_bindings: &mut std::collections::HashMap<&'a str, StillLocalBindingCompileInfo>,
    bindings_to_clone: &mut Vec<BindingToClone<'a>>,
    type_aliases: &std::collections::HashMap<StillName, TypeAliasInfo>,
    choice_types: &std::collections::HashMap<StillName, ChoiceTypeInfo>,
    is_reference: bool,
    maybe_pattern_node: Option<StillSyntaxNode<&'a StillSyntaxPattern>>,
) -> CompiledStillPattern {
    match maybe_pattern_node {
        None => {
            errors.push(error_on_none());
            CompiledStillPattern {
                rust: None,
                type_: None,
            }
        }
        Some(pattern_node) => still_syntax_pattern_to_rust(
            errors,
            records_used,
            introduced_bindings,
            bindings_to_clone,
            type_aliases,
            choice_types,
            is_reference,
            pattern_node,
        ),
    }
}
fn still_syntax_type_to_type(
    errors: &mut Vec<StillErrorNode>,
    type_aliases: &std::collections::HashMap<StillName, TypeAliasInfo>,
    choice_types: &std::collections::HashMap<StillName, ChoiceTypeInfo>,
    type_node: StillSyntaxNode<&StillSyntaxType>,
) -> Option<StillType> {
    match type_node.value {
        StillSyntaxType::Variable(name) => Some(StillType::Variable(name.clone())),
        StillSyntaxType::Parenthesized(maybe_in_parens) => match maybe_in_parens {
            None => {
                errors.push(StillErrorNode {
                    range: type_node.range,
                    message: Box::from("missing type inside these parens (here)"),
                });
                None
            }
            Some(in_parens_node) => still_syntax_type_to_type(
                errors,
                type_aliases,
                choice_types,
                still_syntax_node_unbox(in_parens_node),
            ),
        },
        StillSyntaxType::WithComment {
            comment: _,
            type_: maybe_after_comment,
        } => match maybe_after_comment {
            None => {
                errors.push(StillErrorNode {
                    range: type_node.range,
                    message: Box::from("missing type after this comment # ... \\n here"),
                });
                None
            }
            Some(after_comment_node) => still_syntax_type_to_type(
                errors,
                type_aliases,
                choice_types,
                still_syntax_node_unbox(after_comment_node),
            ),
        },
        StillSyntaxType::Function {
            inputs,
            arrow_key_symbol_range: _,
            output: maybe_output,
        } => {
            let Some(output_node) = maybe_output else {
                errors.push(StillErrorNode {
                    range: type_node.range,
                    message: Box::from(
                        "missing output type after these inputs and arrow \\..inputs.. > here",
                    ),
                });
                return None;
            };
            if inputs.is_empty() {
                errors.push(StillErrorNode {
                    range: type_node.range,
                    message: Box::from("missing input types \\here > ..output.."),
                });
                return None;
            }
            let input_types: Vec<StillType> = inputs
                .iter()
                .map(|input_node| {
                    still_syntax_type_to_type(
                        errors,
                        type_aliases,
                        choice_types,
                        still_syntax_node_as_ref(input_node),
                    )
                })
                .collect::<Option<Vec<_>>>()?;
            let output_type: StillType = still_syntax_type_to_type(
                errors,
                type_aliases,
                choice_types,
                still_syntax_node_unbox(output_node),
            )?;
            Some(StillType::Function {
                inputs: input_types,
                output: Box::new(output_type),
            })
        }
        StillSyntaxType::Construct {
            name: name_node,
            arguments,
        } => {
            let argument_types: Vec<StillType> = arguments
                .iter()
                .map(|argument_node| {
                    still_syntax_type_to_type(
                        errors,
                        type_aliases,
                        choice_types,
                        still_syntax_node_as_ref(argument_node),
                    )
                })
                .collect::<Option<Vec<_>>>()?;
            if let Some(origin_type_alias) = type_aliases.get(&name_node.value) {
                match origin_type_alias.parameters.len().cmp(&arguments.len()) {
                    std::cmp::Ordering::Equal => {}
                    std::cmp::Ordering::Less => {
                        errors.push(StillErrorNode {
                            range: name_node.range,
                            message: format!(
                                "this type alias has {} less parameters than arguments are provided here.",
                                arguments.len() - origin_type_alias.parameters.len(),
                            ).into_boxed_str()
                        });
                        return None;
                    }
                    std::cmp::Ordering::Greater => {
                        errors.push(StillErrorNode {
                            range: name_node.range,
                            message: format!(
                                "this type alias has {} more parameters than arguments are provided here. The additional parameters are called {}",
                                origin_type_alias.parameters.len() - arguments.len(),
                                origin_type_alias.parameters.iter().map(|parameter_node| parameter_node.value.as_str()).skip(arguments.len()).collect::<Vec<_>>().join(", ")
                            ).into_boxed_str()
                        });
                        // later arguments will be ignored
                    }
                }
                return still_type_construct_resolve_type_alias(origin_type_alias, &argument_types);
            }
            let Some(origin_choice_type) = choice_types.get(&name_node.value) else {
                errors.push(StillErrorNode {
                    range: name_node.range,
                    message: Box::from("no type alias or choice type is declared with this name"),
                });
                return None;
            };
            match origin_choice_type.parameters.len().cmp(&arguments.len()) {
                std::cmp::Ordering::Equal => {}
                std::cmp::Ordering::Less => {
                    errors.push(StillErrorNode {
                        range: name_node.range,
                        message: format!(
                            "this choice type has {} less parameters than arguments are provided here.",
                            arguments.len() - origin_choice_type.parameters.len(),
                        ).into_boxed_str()
                    });
                    return None;
                }
                std::cmp::Ordering::Greater => {
                    errors.push(StillErrorNode {
                        range: name_node.range,
                        message: format!(
                            "this choice type has {} more parameters than arguments are provided here. The additional parameters are called {}",
                            origin_choice_type.parameters.len() - arguments.len(),
                            origin_choice_type.parameters.iter().map(|parameter_node| parameter_node.value.as_str()).skip(arguments.len()).collect::<Vec<_>>().join(", ")
                        ).into_boxed_str()
                    });
                    // later arguments will be ignored
                }
            }
            Some(StillType::ChoiceConstruct {
                name: name_node.value.clone(),
                arguments: argument_types,
            })
        }
        StillSyntaxType::Record(fields) => {
            // TODO fail if contains duplicate
            let field_types: Vec<StillTypeField> = fields
                .iter()
                .map(|field| match &field.value {
                    None => {
                        errors.push(StillErrorNode {
                            range: field.name.range,
                            message: Box::from(
                                "missing field value after this name ..field-name.. here",
                            ),
                        });
                        None
                    }
                    Some(field_value_node) => {
                        let field_value_type: StillType = still_syntax_type_to_type(
                            errors,
                            type_aliases,
                            choice_types,
                            still_syntax_node_as_ref(field_value_node),
                        )?;
                        Some(StillTypeField {
                            name: field.name.value.clone(),
                            value: field_value_type,
                        })
                    }
                })
                .collect::<Option<Vec<_>>>()?;
            Some(StillType::Record(field_types))
        }
    }
}
struct BindingToClone<'a> {
    name: &'a str,
    is_copy: bool,
}
/// TODO should be `Option<{ type_: StillSype,  }>`
/// as an untyped pattern should never exist
struct CompiledStillPattern {
    // None means it should be ignored (e.g. in a case of that case should be removed)
    rust: Option<syn::Pat>,
    type_: Option<StillType>,
}
fn still_syntax_pattern_to_rust<'a>(
    errors: &mut Vec<StillErrorNode>,
    records_used: &mut std::collections::HashSet<Vec<StillName>>,
    introduced_bindings: &mut std::collections::HashMap<&'a str, StillLocalBindingCompileInfo>,
    bindings_to_clone: &mut Vec<BindingToClone<'a>>,
    type_aliases: &std::collections::HashMap<StillName, TypeAliasInfo>,
    choice_types: &std::collections::HashMap<StillName, ChoiceTypeInfo>,
    is_reference: bool,
    pattern_node: StillSyntaxNode<&'a StillSyntaxPattern>,
) -> CompiledStillPattern {
    match &pattern_node.value {
        StillSyntaxPattern::Char(maybe_char) => CompiledStillPattern {
            type_: Some(still_type_chr),
            rust: match *maybe_char {
                None => {
                    errors.push(StillErrorNode {
                        range: pattern_node.range,
                        message: Box::from("missing character between 'here'"),
                    });
                    None
                }
                Some(char_value) => Some(syn::Pat::Lit(syn::ExprLit {
                    attrs: vec![],
                    lit: syn::Lit::Char(syn::LitChar::new(char_value, syn_span())),
                })),
            },
        },
        StillSyntaxPattern::Int(representation) => CompiledStillPattern {
            type_: Some(still_type_int),
            rust: match representation.parse::<isize>() {
                Ok(int) => Some(syn::Pat::Lit(syn::ExprLit {
                    attrs: vec![],
                    lit: syn::Lit::Int(syn::LitInt::new(&int.to_string(), syn_span())),
                })),
                Err(parse_error) => {
                    errors.push(StillErrorNode {
                        range: pattern_node.range,
                        message: format!(
                            "invalid int format. Expected base 10 whole number like -123 or 0: {parse_error}"
                        ).into_boxed_str(),
                    });
                    None
                }
            },
        },
        StillSyntaxPattern::String {
            content,
            quoting_style: _,
        } => CompiledStillPattern {
            type_: Some(still_type_str),
            rust: Some(syn::Pat::Lit(syn::ExprLit {
                attrs: vec![],
                lit: syn::Lit::Str(syn::LitStr::new(content, syn_span())),
            })),
        },
        StillSyntaxPattern::WithComment {
            comment: _,
            pattern: maybe_after_comment,
        } => maybe_still_syntax_pattern_to_rust(
            errors,
            || StillErrorNode {
                range: pattern_node.range,
                message: Box::from("missing pattern after comment # ...\\n here"),
            },
            records_used,
            introduced_bindings,
            bindings_to_clone,
            type_aliases,
            choice_types,
            is_reference,
            maybe_after_comment.as_ref().map(still_syntax_node_unbox),
        ),
        StillSyntaxPattern::Typed {
            type_: maybe_type_node,
            pattern: maybe_in_typed,
        } => {
            let maybe_type: Option<StillType> = match maybe_type_node {
                None => {
                    errors.push(StillErrorNode {
                        range: pattern_node.range,
                        message: Box::from("missing type between :here:"),
                    });
                    None
                }
                Some(type_node) => still_syntax_type_to_type(
                    errors,
                    type_aliases,
                    choice_types,
                    still_syntax_node_as_ref(type_node),
                ),
            };
            match maybe_in_typed {
                None => {
                    errors.push(StillErrorNode {
                        range: pattern_node.range,
                        message: Box::from("missing pattern after type :...: here. To ignore he incoming value, use _, or give it a lowercase name or specify a variant"),
                    });
                    CompiledStillPattern {
                        rust: Some(syn_pat_wild()),
                        type_: maybe_type,
                    }
                }
                Some(untyped_pattern_node) => match &untyped_pattern_node.value {
                    StillSyntaxPatternUntyped::Variable(name) => {
                        let maybe_existing_pattern_variable_with_same_name_info: Option<
                            StillLocalBindingCompileInfo,
                        > = introduced_bindings.insert(
                            name,
                            StillLocalBindingCompileInfo {
                                origin_range: untyped_pattern_node.range,
                                is_copy: maybe_type.as_ref().is_some_and(|type_| {
                                    still_type_is_copy(false, type_aliases, choice_types, type_)
                                }),
                                type_: maybe_type.clone(),
                                last_uses: vec![],
                                closures_it_is_used_in: vec![],
                            },
                        );
                        if maybe_existing_pattern_variable_with_same_name_info.is_some() {
                            errors.push(StillErrorNode {
                                range: untyped_pattern_node.range,
                                message: Box::from("a variable with this name is already used in another part of the patterns. Rename one of them")
                            });
                        }
                        if is_reference {
                            bindings_to_clone.push(BindingToClone {
                                name: name,
                                is_copy: maybe_type.as_ref().is_some_and(|type_| {
                                    still_type_is_copy(false, type_aliases, choice_types, type_)
                                }),
                            });
                        }
                        CompiledStillPattern {
                            rust: Some(syn_pat_variable(name)),
                            type_: maybe_type,
                        }
                    }
                    StillSyntaxPatternUntyped::Ignored => CompiledStillPattern {
                        rust: Some(syn_pat_wild()),
                        type_: maybe_type,
                    },
                    StillSyntaxPatternUntyped::Variant {
                        name: name_node,
                        value: maybe_value,
                    } => {
                        let Some(type_) = maybe_type else {
                            return CompiledStillPattern {
                                rust: None,
                                type_: None,
                            };
                        };
                        let StillType::ChoiceConstruct {
                            name: origin_choice_type_name,
                            arguments: origin_choice_type_arguments,
                        } = type_
                        else {
                            errors.push(StillErrorNode {
                                range: maybe_type_node.as_ref().map(|n| n.range).unwrap_or(pattern_node.range),
                                message: Box::from("type in :here: is not a choice type which is necessary for a variant pattern"),
                            });
                            return CompiledStillPattern {
                                rust: None,
                                type_: None,
                            };
                        };
                        let variant_value_is_reference: bool = is_reference
                            || ('variant_value_is_reference: {
                                let Some(origin_choice_type) =
                                    choice_types.get(origin_choice_type_name.as_str())
                                else {
                                    break 'variant_value_is_reference false;
                                };
                                let Some(variant_index_in_origin_choice_type) = origin_choice_type
                                    .variants
                                    .iter()
                                    .enumerate()
                                    .find(|(_, origin_choice_type_variant)| {
                                        origin_choice_type_variant.name.as_ref().is_some_and(
                                            |origin_choice_type_variant_name_node| {
                                                origin_choice_type_variant_name_node.value
                                                    == name_node.value
                                            },
                                        )
                                    })
                                    .map(|(i, _)| i)
                                else {
                                    break 'variant_value_is_reference false;
                                };
                                origin_choice_type
                                    .type_variants
                                    .get(variant_index_in_origin_choice_type)
                                    .and_then(|type_variant| type_variant.value.as_ref())
                                    .is_some_and(|variant_value| {
                                        variant_value.constructs_recursive_type
                                    })
                            });
                        let maybe_rust_value: Option<syn::Pat> = match maybe_value.as_ref() {
                            None => {
                                // TODO check origin variant also has no value
                                None
                            }
                            Some(value_node) => {
                                let compiled_value = still_syntax_pattern_to_rust(
                                    errors,
                                    records_used,
                                    introduced_bindings,
                                    bindings_to_clone,
                                    type_aliases,
                                    choice_types,
                                    variant_value_is_reference,
                                    still_syntax_node_unbox(value_node),
                                );
                                let Some(value_rust_pattern) = compiled_value.rust else {
                                    return CompiledStillPattern {
                                        rust: None,
                                        type_: Some(StillType::ChoiceConstruct {
                                            name: origin_choice_type_name,
                                            arguments: origin_choice_type_arguments,
                                        }),
                                    };
                                };
                                let Some(_value_type) = compiled_value.type_ else {
                                    return CompiledStillPattern {
                                        rust: None,
                                        type_: Some(StillType::ChoiceConstruct {
                                            name: origin_choice_type_name,
                                            arguments: origin_choice_type_arguments,
                                        }),
                                    };
                                };
                                // TODO verify equal: origin choice type variant value with the type arguments inlined & value pattern type
                                Some(value_rust_pattern)
                            }
                        };
                        let rust_variant_path = syn_path_reference([
                            &still_name_to_uppercase_rust(&origin_choice_type_name),
                            &still_name_to_uppercase_rust(&name_node.value),
                        ]);
                        CompiledStillPattern {
                            rust: Some(match maybe_rust_value {
                                None => syn::Pat::Path(syn::ExprPath {
                                    attrs: vec![],
                                    qself: None,
                                    path: rust_variant_path,
                                }),
                                Some(rust_value) => syn::Pat::TupleStruct(syn::PatTupleStruct {
                                    attrs: vec![],
                                    qself: None,
                                    path: rust_variant_path,
                                    paren_token: syn::token::Paren(syn_span()),
                                    elems: std::iter::once(rust_value).collect(),
                                }),
                            }),
                            type_: Some(StillType::ChoiceConstruct {
                                name: origin_choice_type_name,
                                arguments: origin_choice_type_arguments,
                            }),
                        }
                    }
                },
            }
        }
        StillSyntaxPattern::Record(fields) => {
            // TODO check for duplictes
            records_used.insert(sorted_field_names(
                fields.iter().map(|field| &field.name.value),
            ));
            let (field_values_rust, field_types): (
                Vec<Option<(&str, syn::Pat)>>,
                Vec<Option<StillTypeField>>,
            ) = fields
                .iter()
                .map(|field| {
                    let compiled_field_value: CompiledStillPattern =
                        maybe_still_syntax_pattern_to_rust(
                            errors,
                            || StillErrorNode {
                                range: field.name.range,
                                message: Box::from("missing field value after this name"),
                            },
                            records_used,
                            introduced_bindings,
                            bindings_to_clone,
                            type_aliases,
                            choice_types,
                            is_reference,
                            field.value.as_ref().map(still_syntax_node_as_ref),
                        );
                    (
                        compiled_field_value
                            .rust
                            .map(|rust| (field.name.value.as_str(), rust)),
                        compiled_field_value.type_.map(|type_| StillTypeField {
                            name: field.name.value.clone(),
                            value: type_,
                        }),
                    )
                })
                .unzip();
            CompiledStillPattern {
                type_: field_types
                    .into_iter()
                    .collect::<Option<Vec<StillTypeField>>>()
                    .map(|type_fields| StillType::Record(type_fields)),
                rust: field_values_rust
                    .into_iter()
                    .collect::<Option<Vec<_>>>()
                    .map(|field_values_rust| {
                        syn::Pat::Struct(syn::PatStruct {
                            attrs: vec![],
                            qself: None,
                            path: syn_path_reference([
                                &still_field_names_to_rust_record_struct_name(
                                    fields.iter().map(|field| field.name.value.as_ref()),
                                ),
                            ]),
                            brace_token: syn::token::Brace(syn_span()),
                            fields: field_values_rust
                                .into_iter()
                                .map(|(field_name, field_value_rust)| syn::FieldPat {
                                    attrs: vec![],
                                    member: syn::Member::Named(syn_ident(
                                        &still_name_to_lowercase_rust(field_name),
                                    )),
                                    colon_token: Some(syn::token::Colon(syn_span())),
                                    pat: Box::new(field_value_rust),
                                })
                                .collect(),
                            rest: None,
                        })
                    }),
            }
        }
    }
}
fn bindings_to_clone_to_rust_into(
    rust_stmts: &mut Vec<syn::Stmt>,
    bindings_to_clone: Vec<BindingToClone>,
) {
    rust_stmts.extend(bindings_to_clone.into_iter().map(|binding_to_clone| {
        let rust_expr_binding_reference: syn::Expr = syn_expr_reference([binding_to_clone.name]);
        syn::Stmt::Local(syn::Local {
            attrs: vec![],
            let_token: syn::token::Let(syn_span()),
            pat: syn_pat_variable(binding_to_clone.name),
            init: Some(syn::LocalInit {
                eq_token: syn::token::Eq(syn_span()),
                expr: Box::new(if binding_to_clone.is_copy {
                    syn::Expr::Unary(syn::ExprUnary {
                        attrs: vec![],
                        op: syn::UnOp::Deref(syn::token::Star(syn_span())),
                        expr: Box::new(rust_expr_binding_reference),
                    })
                } else {
                    syn_expr_call_clone_method(rust_expr_binding_reference)
                }),
                diverge: None,
            }),
            semi_token: syn::token::Semi(syn_span()),
        })
    }));
}
fn still_name_to_uppercase_rust(name: &str) -> String {
    let mut sanitized: String = name.replace("-", "_");
    if let Some(first) = sanitized.get_mut(0..=0) {
        first.make_ascii_uppercase();
    }
    if [
        "Self",
        "Clone",
        "Copy",
        "PartialEq",
        "Eq",
        "Debug",
        "Hash",
        "PartialOrd",
        "Ord",
        "Blank",
        "Alloc",
        "StillIntoOwned",
        "OwnedToStill",
        "Fn",
        // type variables used in core
        "A",
        "N",
        "Continue",
        "Exit",
        "Inputs",
        "Output",
        "State",
    ]
    .contains(&sanitized.as_str())
    {
        sanitized + "ø_"
    } else {
        sanitized
    }
}
fn still_name_to_lowercase_rust(name: &str) -> String {
    let mut sanitized: String = name.replace("-", "_");
    if let Some(first) = sanitized.get_mut(0..=0) {
        first.make_ascii_lowercase();
    }
    if rust_lowercase_keywords.contains(&sanitized.as_str()) || sanitized == "vec_literal" {
        sanitized + "ø"
    } else {
        sanitized
    }
}
/// both weak, reserved and strong.
/// see <https://doc.rust-lang.org/reference/keywords.html>
const rust_lowercase_keywords: [&str; 55] = [
    "as",
    "break",
    "const",
    "continue",
    "crate",
    "else",
    "enum",
    "extern",
    "false",
    "fn",
    "for",
    "if",
    "impl",
    "in",
    "let",
    "loop",
    "match",
    "mod",
    "move",
    "mut",
    "pub",
    "ref",
    "return",
    "self",
    "struct",
    "super",
    "trait",
    "true",
    "type",
    "unsafe",
    "use",
    "where",
    "while",
    "async",
    "await",
    "dyn",
    "abstract",
    "become",
    "box",
    "do",
    "final",
    "macro",
    "override",
    "priv",
    "typeof",
    "unsized",
    "virtual",
    "yield",
    "try",
    "gen",
    "static",
    "macro_rules",
    "raw",
    "safe",
    "union",
];
fn still_type_variable_to_rust(name: &str) -> String {
    // to disambiguate from choice type and type alias names
    still_name_to_uppercase_rust(name) + "ø"
}
fn still_field_names_to_rust_record_struct_name<'a>(
    field_names: impl Iterator<Item = &'a str>,
) -> String {
    let mut rust_field_names_vec: Vec<String> = field_names
        .map(still_name_to_lowercase_rust)
        .collect::<Vec<_>>();
    rust_field_names_vec.sort();
    /*
    field names in the final type can
    not just separated by _ or __ because still identifiers are
    allowed to contain multiple consecutive -s.

    Below solution would work without harder to type
    separator unicode characters.
    However, it is also less performant, creates longer, uglier names and doesn't disambiguate
    from choice type and type alias names:

    let consecutive_underscore_count: usize = rust_field_names_vec
        .iter()
        .filter_map(|rust_field_name| {
            // credits for the idea: https://users.rust-lang.org/t/returning-maximum-number-of-consecutive-1s-in-list-of-binary-numbers/56717/6
            rust_field_name.split(|c| c != '_').map(str::len).max()
        })
        .max()
        .unwrap_or(0);

    and joined with

    &"_".repeat(consecutive_underscore_count + 1)
    */
    // the separator between fields is the "middle dot": https://util.unicode.org/UnicodeJsps/character.jsp?a=00B7
    // It is chosen because
    // - it can be typed on regular keyboards (on my keyboard at least it's AltGr+., on mac it seems to be Option+Shift+9, not sure for the rest.
    //   if it cannot be typed on your keyboard, please open an issue!)
    // - it looks similar to the field access dot
    // - it is somewhat commonly understood as a separator
    let mut field_names_joined: String = rust_field_names_vec.join("·");
    match field_names_joined.get_mut(0..=0) {
        Some(first) => {
            first.make_ascii_uppercase();
            if rust_field_names_vec.len() == 1 {
                field_names_joined.push('·');
            }
            field_names_joined
        }
        None => "Blank".to_string(),
    }
}
/// seems dumb
fn syn_span() -> proc_macro2::Span {
    proc_macro2::Span::call_site()
}
fn syn_lifetime(name: &str) -> syn::Lifetime {
    syn::Lifetime {
        apostrophe: syn_span(),
        ident: syn_ident(name),
    }
}
const syn_default_lifetime_name: &str = "a";
const syn_static_lifetime_name: &str = "static";
fn syn_default_lifetime() -> syn::Lifetime {
    syn_lifetime(syn_default_lifetime_name)
}
fn syn_default_lifetime_param() -> syn::LifetimeParam {
    syn::LifetimeParam::new(syn_default_lifetime())
}
fn syn_ident(name: &str) -> syn::Ident {
    syn::Ident::new(name, syn_span())
}
fn syn_path_reference<const N: usize>(segments: [&str; N]) -> syn::Path {
    syn::Path {
        leading_colon: None,
        segments: segments
            .into_iter()
            .map(|name| syn_path_segment_ident(name))
            .collect(),
    }
}
fn syn_path_segment_ident(name: &str) -> syn::PathSegment {
    syn::PathSegment {
        ident: syn_ident(name),
        arguments: syn::PathArguments::None,
    }
}
fn syn_path_name_with_arguments(
    name: &str,
    generic_arguments: impl Iterator<Item = syn::GenericArgument>,
) -> syn::Path {
    syn::Path {
        leading_colon: None,
        segments: std::iter::once(syn::PathSegment {
            ident: syn_ident(name),
            arguments: syn::PathArguments::AngleBracketed(syn::AngleBracketedGenericArguments {
                colon2_token: None,
                lt_token: syn::token::Lt(syn_span()),
                args: generic_arguments.collect(),
                gt_token: syn::token::Gt(syn_span()),
            }),
        })
        .collect(),
    }
}
fn syn_attribute_doc(documentation: &str) -> syn::Attribute {
    syn::Attribute {
        pound_token: syn::token::Pound(syn_span()),
        style: syn::AttrStyle::Outer,
        bracket_token: syn::token::Bracket(syn_span()),
        meta: syn::Meta::NameValue(syn::MetaNameValue {
            path: syn::Path::from(syn_ident("doc")),
            eq_token: syn::token::Eq(syn_span()),
            value: syn::Expr::Lit(syn::ExprLit {
                attrs: vec![],
                lit: syn::Lit::Str(syn::LitStr::new(documentation, syn_span())),
            }),
        }),
    }
}
fn syn_generics_none() -> syn::Generics {
    syn::Generics {
        lt_token: None,
        params: syn::punctuated::Punctuated::new(),
        gt_token: None,
        where_clause: None,
    }
}
fn syn_pat_wild() -> syn::Pat {
    syn::Pat::Wild(syn::PatWild {
        attrs: vec![],
        underscore_token: syn::token::Underscore(syn_span()),
    })
}
fn syn_pat_variable(name: &str) -> syn::Pat {
    syn::Pat::Ident(syn::PatIdent {
        attrs: vec![],
        by_ref: None,
        mutability: None,
        ident: syn_ident(&still_name_to_lowercase_rust(name)),
        subpat: None,
    })
}
fn syn_type_variable(name: &str) -> syn::Type {
    syn::Type::Path(syn::TypePath {
        qself: None,
        path: syn::Path::from(syn_ident(name)),
    })
}
fn default_parameter_bounds(lifetime: &str) -> impl Iterator<Item = syn::TypeParamBound> {
    [
        syn::TypeParamBound::Lifetime(syn_lifetime(lifetime)),
        syn::TypeParamBound::Trait(syn::TraitBound {
            paren_token: None,
            modifier: syn::TraitBoundModifier::None,
            lifetimes: None,
            path: syn::Path::from(syn_ident("Clone")),
        }),
    ]
    .into_iter()
}
fn default_dyn_fn_bounds(lifetime: &str) -> impl Iterator<Item = syn::TypeParamBound> {
    [syn::TypeParamBound::Lifetime(syn_lifetime(lifetime))].into_iter()
}
fn syn_attribute_derive<'a>(trait_macro_names: impl Iterator<Item = &'a str>) -> syn::Attribute {
    syn::Attribute {
        pound_token: syn::token::Pound(syn_span()),
        style: syn::AttrStyle::Outer,
        bracket_token: syn::token::Bracket(syn_span()),
        meta: syn::Meta::List(syn::MetaList {
            path: syn_path_reference(["derive"]),
            delimiter: syn::MacroDelimiter::Paren(syn::token::Paren(syn_span())),
            // is there really no way to print e.g. Punctuated?
            tokens: trait_macro_names
                .flat_map(|token| {
                    [
                        proc_macro2::TokenTree::Ident(syn_ident(token)),
                        proc_macro2::TokenTree::Punct(proc_macro2::Punct::new(
                            ',',
                            proc_macro2::Spacing::Alone,
                        )),
                    ]
                })
                .collect(),
        }),
    }
}
const default_allocator_parameter_name: &str = "alloc";
fn default_allocator_fn_arg() -> syn::FnArg {
    syn::FnArg::Typed(syn::PatType {
        attrs: vec![],
        pat: Box::new(syn_pat_variable(default_allocator_parameter_name)),
        colon_token: syn::token::Colon(syn_span()),
        ty: Box::new(syn::Type::Reference(syn::TypeReference {
            and_token: syn::token::And(syn_span()),
            mutability: None,
            lifetime: Some(syn_default_lifetime()),
            elem: Box::new(syn::Type::ImplTrait(syn::TypeImplTrait {
                impl_token: syn::token::Impl(syn_span()),
                bounds: std::iter::once(syn::TypeParamBound::Trait(syn::TraitBound {
                    paren_token: None,
                    modifier: syn::TraitBoundModifier::None,
                    lifetimes: None,
                    path: syn_path_reference(["Alloc"]),
                }))
                .collect(),
            })),
        })),
    })
}
fn syn_expr_call_alloc_method(to_allocate: syn::Expr) -> syn::Expr {
    syn::Expr::MethodCall(syn::ExprMethodCall {
        attrs: vec![],
        receiver: Box::new(syn_expr_reference([default_allocator_parameter_name])),
        dot_token: syn::token::Dot(syn_span()),
        method: syn_ident("alloc"),
        turbofish: None,
        paren_token: syn::token::Paren(syn_span()),
        args: std::iter::once(to_allocate).collect(),
    })
}
fn syn_expr_call_clone_method(to_clone: syn::Expr) -> syn::Expr {
    syn::Expr::MethodCall(syn::ExprMethodCall {
        attrs: vec![],
        receiver: Box::new(to_clone),
        dot_token: syn::token::Dot(syn_span()),
        method: syn_ident("clone"),
        turbofish: None,
        paren_token: syn::token::Paren(syn_span()),
        args: syn::punctuated::Punctuated::new(),
    })
}
fn syn_expr_todo() -> syn::Expr {
    syn::Expr::Macro(syn::ExprMacro {
        attrs: vec![],
        mac: syn::Macro {
            path: syn_path_reference(["std", "todo"]),
            bang_token: syn::token::Not(syn_span()),
            delimiter: syn::MacroDelimiter::Paren(syn::token::Paren(syn_span())),
            tokens: proc_macro2::TokenStream::new(),
        },
    })
}
fn syn_expr_reference<const N: usize>(segments: [&str; N]) -> syn::Expr {
    syn::Expr::Path(syn::ExprPath {
        attrs: vec![],
        qself: None,
        path: syn_path_reference(segments),
    })
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
