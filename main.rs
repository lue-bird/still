#![allow(non_upper_case_globals)]
use lily_compile as lily;

struct State {
    projects: std::collections::HashMap<lsp_types::Uri, ProjectState>,
}
struct ProjectState {
    source: String,
    syntax: lily::SyntaxProject,
    type_aliases: std::collections::HashMap<lily::Name, lily::TypeAliasInfo>,
    choice_types: std::collections::HashMap<lily::Name, lily::ChoiceTypeInfo>,
    variable_declarations:
        std::collections::HashMap<lily::Name, lily::CompiledVariableDeclarationInfo>,
    records: std::collections::HashSet<Vec<lily::Name>>,
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut full_command = std::env::args().skip(1);
    match full_command.next() {
        None => {
            // consider a help message instead
            lsp_main()
        }
        Some(command) => match command.as_str() {
            "lsp" | "language-server" | "ls" => lsp_main(),
            "help" | "-h" | "--help" | "elp" | "h" | "pad" | "?" | "-?" | "--h" | "--?" => {
                println!("{command_help}");
                Ok(())
            }
            "build" | "make" | "compile" | "transpile" | "b" | "m" | "c" => {
                let maybe_input_file_path: Option<String> = full_command.next();
                let maybe_output_file_path: Option<String> = full_command.next();
                build_main(
                    maybe_input_file_path.as_ref().map(std::path::Path::new),
                    maybe_output_file_path.as_ref().map(std::path::Path::new),
                );
                Ok(())
            }
            "doc" | "docs" | "documentation" | "core" | "stdlib" | "core-doc" | "core-docs"
            | "core-documentation" | "core-types" | "d" => {
                println!("Here are all core declarations:\n");
                print_core_lily_docs();
                Ok(())
            }
            "init" | "initialize" | "new" | "create" | "setup" | "boilerplate" | "template"
            | "hello" | "hello-world" => {
                println!(
                    "Each project has one .lily file. For applications, a rust project is also needed. Both will be initialized now."
                );
                if full_command.next().is_some() {
                    println!(
                        "Nothing was created. If you want to initialize a lily project in a directory, please create that directory yourself and run lily init from inside there."
                    );
                    return Ok(());
                }
                initialize_new_lily_hello_world_project();
                Ok(())
            }
            _ => {
                println!("Unknown command name.\n{command_help}");
                Ok(())
            }
        },
    }
}
const command_help: &str = "\
To compile to a rust file: lily build [input-file.lily [output-file.rs]]
To copy the hello-world project setup into the current directory: lily init
To start the language server: lily lsp
To print core declaration documentation: lily core-docs
To run a rust project: cargo run
To compile a rust project into an executable: cargo build --release
To print this help message: lily help
See the source code, see the full documentation, report bugs or leave any kind of feedback at https://codeberg.org/lue-bird/lily";

fn print_core_lily_docs() {
    for (core_choice_type_name, core_choice_type_info) in lily::core_choice_type_infos.iter() {
        let mut declaration_string: String = String::new();
        lily::syntax_choice_type_declaration_into(
            &mut declaration_string,
            Some(core_choice_type_name),
            &core_choice_type_info.parameters,
            &core_choice_type_info.variants,
        );
        println!("{}", declaration_string);
        if let Some(documentation) = &core_choice_type_info.documentation {
            println!(
                "{}",
                documentation_comment_to_markdown(documentation)
                    .lines()
                    .fold(String::new(), |so_far, line| so_far + "    " + line + "\n")
            );
        }
    }
    for (core_variable_name, core_variable_info) in lily::core_variable_declaration_infos.iter() {
        match &core_variable_info.type_ {
            Some(variable_type) => {
                let mut type_string: String = String::new();
                lily::type_info_into(&mut type_string, 5, variable_type);
                print!(
                    "{core_variable_name}
    :{type_string}{}:",
                    if type_string.contains('\n') {
                        "\n    "
                    } else {
                        ""
                    },
                );
            }
            None => {
                print!("{core_variable_name}");
            }
        }
        if let Some(documentation) = &core_variable_info.documentation {
            println!(
                "\n{}",
                documentation_comment_to_markdown(documentation)
                    .lines()
                    .fold(String::new(), |so_far, line| so_far + "    " + line + "\n")
            );
        }
    }
}
fn initialize_new_lily_hello_world_project() {
    try_generate_file(
        "lily.lily",
        "this is where all your lily code goes",
        r#"

greet \:str:name >
    strs-flatten [ "Hello, ", name, "\n" ]

"#,
    );
    try_generate_file(
        "main.rs",
        "the actual program entrypoint, written in rust.",
        r#"// enabling deref_patterns is sadly required for matching recursive choice types
#![feature(deref_patterns)]
#![allow(incomplete_features)]

mod lily;

fn main() {
    print!("{}", lily::greet(lily::Str::Slice("world")));
}
"#,
    );
    try_generate_file(
        "Cargo.toml",
        "this tells cargo (the rust package manager) how to build the project",
        r#"[package]
name = "example"
edition = "2024"
[[bin]]
name = "example"
path = "main.rs"
"#,
    );
    try_generate_file(
        "rust-toolchain.toml",
        "this allows rust tooling to build the project with nightly features",
        r#"[toolchain]
channel = "nightly"
"#,
    );
    try_generate_file(
        ".gitignore",
        "this tells git to not track the generated rust code",
        r"# Generated rust code
lily/
",
    );
    match std::fs::exists("lily") {
        Ok(true) => {
            println!("lily/ directory already exists, skipping generating it.");
        }
        Ok(false) => {
            let write_result: Result<(), std::io::Error> = std::fs::create_dir("lily");
            match write_result {
                Ok(()) => {
                    println!(
                        "created lily/ directory, this will contain the generated rust file lily/mod.rs."
                    );
                }
                Err(error) => {
                    println!("failed to generate lily/ directory: {error}");
                }
            }
        }
        Err(error) => {
            println!("failed to check if lily/ directory already exists: {error}");
        }
    }
}
fn try_generate_file(path: &str, purpose: &str, content: &str) {
    match std::fs::exists(path) {
        Ok(true) => {
            println!("{path} already exists, skipping generating it.");
        }
        Ok(false) => {
            let write_result: Result<(), std::io::Error> = std::fs::write(path, content);
            match write_result {
                Ok(()) => {
                    println!("created {path}, {purpose}.");
                }
                Err(error) => {
                    println!("failed to generate {path}: {error}");
                }
            }
        }
        Err(error) => {
            println!("failed to check if {path} already exists: {error}");
        }
    }
}
fn default_lily_output_file_path_for_input_file_path(
    input_file_path: &std::path::Path,
) -> std::path::PathBuf {
    std::path::Path::join(&input_file_path.with_extension(""), "mod.rs")
}

fn build_main(
    maybe_input_file_path: Option<&std::path::Path>,
    maybe_output_file_path: Option<&std::path::Path>,
) {
    let input_file_path: &std::path::Path = match maybe_input_file_path {
        Some(input_file_path) => &input_file_path.with_extension("lily"),
        None => std::path::Path::new("lily.lily"),
    };
    let output_file_path: &std::path::Path = match maybe_output_file_path {
        Some(output_file_path) => &output_file_path.with_extension(".rs"),
        None => &default_lily_output_file_path_for_input_file_path(input_file_path),
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
            let syntax_project: lily::SyntaxProject = lily::parse_syntax_project(&project_source);
            let mut output_errors: Vec<lily::ErrorNode> = Vec::new();
            let compiled_project: lily::CompiledProject =
                lily::project_compile_to_rust(&mut output_errors, &syntax_project);
            for output_error in &output_errors {
                eprintln!(
                    "{input_file_path:?}:{range_start_line}:{range_start_column} {message}",
                    range_start_line = output_error.range.start.line + 1,
                    range_start_column = output_error.range.start.character + 1,
                    message = &output_error.message
                );
            }
            let output_rust_file_string: String =
                lily::compiled_rust_to_file_content(&compiled_project.rust);
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

    let (initialize_request_id, _initialize_arguments_json) = connection.initialize_start()?;
    connection.initialize_finish(
        initialize_request_id,
        serde_json::to_value(lsp_types::InitializeResult {
            capabilities: server_capabilities(),
            server_info: Some(lsp_types::ServerInfo {
                name: "lily".to_string(),
                version: Some("0.0.1".to_string()),
            }),
        })?,
    )?;
    let state: State = initial_state();
    server_loop(&connection, state)?;
    // shut down gracefully
    drop(connection);
    io_thread.join()?;
    Ok(())
}
fn initial_state() -> State {
    State {
        projects: std::collections::HashMap::with_capacity(1),
    }
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
            publish_diagnostics(
                connection,
                lsp_types::PublishDiagnosticsParams {
                    uri: arguments.text_document.uri,
                    diagnostics: vec![],
                    version: None,
                },
            );
        }
        <lsp_types::notification::DidChangeTextDocument as lsp_types::notification::Notification>::METHOD => {
            let arguments: <lsp_types::notification::DidChangeTextDocument as lsp_types::notification::Notification>::Params =
                serde_json::from_value(notification_arguments_json)?;
            update_state_on_did_change_text_document(state, connection, arguments);
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
    if arguments.text_document.language_id == "lily"
        || lsp_uri_to_file_path(&arguments.text_document.uri)
            .is_some_and(|file_path| file_path.extension().is_some_and(|ext| ext == "lily"))
    {
        state.projects.insert(
            arguments.text_document.uri.clone(),
            initialize_project_state_from_source(
                connection,
                arguments.text_document.uri,
                arguments.text_document.text,
            ),
        );
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
                respond_to_references(state, &arguments);
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
    if let Some(project_state) = state
        .projects
        .get_mut(&did_change_text_document.text_document.uri)
    {
        let mut updated_source: String = std::mem::take(&mut project_state.source);
        for change in did_change_text_document.content_changes {
            match (change.range, change.range_length) {
                // means full replacement
                (None, None) => {
                    updated_source = change.text;
                }
                // zed for example does not send a range length
                (Some(range), None) => {
                    string_replace_lsp_range(&mut updated_source, range, &change.text);
                }
                // sending a range is deprecated but e.g. vscode still sends it
                // which allows us to do a faster string replace
                (Some(range), Some(range_length)) => {
                    string_replace_lsp_range_for_length(
                        &mut updated_source,
                        range,
                        range_length as usize,
                        &change.text,
                    );
                }
                (None, Some(_)) => {}
            }
        }
        *project_state = initialize_project_state_from_source(
            connection,
            did_change_text_document.text_document.uri,
            updated_source,
        );
    }
}

fn initialize_project_state_from_source(
    connection: &lsp_server::Connection,
    uri: lsp_types::Uri,
    source: String,
) -> ProjectState {
    let mut errors: Vec<lily::ErrorNode> = Vec::new();
    let parsed_project: lily::SyntaxProject = lily::parse_syntax_project(&source);
    let compiled_project: lily::CompiledProject =
        lily::project_compile_to_rust(&mut errors, &parsed_project);
    if let Some(input_file_path) = lsp_uri_to_file_path(&uri)
        && std::fs::exists(input_file_path.with_extension("")).is_ok_and(|exists| exists)
    {
        let _: std::io::Result<()> = std::fs::write(
            default_lily_output_file_path_for_input_file_path(&input_file_path),
            lily::compiled_rust_to_file_content(&compiled_project.rust),
        );
    }
    publish_diagnostics(
        connection,
        lsp_types::PublishDiagnosticsParams {
            uri,
            diagnostics: errors
                .iter()
                .map(lily_error_node_to_diagnostic)
                .collect::<Vec<_>>(),
            version: None,
        },
    );
    ProjectState {
        source: source,
        type_aliases: compiled_project.type_aliases,
        choice_types: compiled_project.choice_types,
        variable_declarations: compiled_project.variable_declarations,
        records: compiled_project.records,
        syntax: parsed_project,
    }
}

fn respond_to_hover(
    state: &State,
    hover_arguments: &lsp_types::HoverParams,
) -> Option<lsp_types::Hover> {
    let hovered_project_state = state.projects.get(
        &hover_arguments
            .text_document_position_params
            .text_document
            .uri,
    )?;
    let hovered_symbol_node: lily::SyntaxNode<LilySyntaxSymbol> =
        lily_syntax_project_find_symbol_at_position(
            &hovered_project_state.syntax,
            &hovered_project_state.type_aliases,
            &hovered_project_state.choice_types,
            &hovered_project_state.variable_declarations,
            hover_arguments.text_document_position_params.position,
        )?;
    match hovered_symbol_node.value {
        LilySyntaxSymbol::TypeVariable { .. } => None,
        LilySyntaxSymbol::ProjectDeclarationName {
            name: hovered_declaration_name,
            documentation,
            declaration: declaration_node,
        } => {
            let origin_declaration_info_markdown: String = match &declaration_node.value {
                lily::SyntaxDeclaration::ChoiceType {
                    name: origin_project_declaration_maybe_name,
                    parameters: origin_project_declaration_parameters,
                    variants: origin_project_declaration_variants,
                } => {
                    format!(
                        "{}{}",
                        if origin_project_declaration_maybe_name
                            .as_ref()
                            .is_some_and(|node| &node.value == hovered_declaration_name)
                        {
                            ""
                        } else {
                            "variant in\n"
                        },
                        &present_choice_type_declaration_info_markdown(
                            origin_project_declaration_maybe_name
                                .as_ref()
                                .map(|n| &n.value),
                            documentation,
                            origin_project_declaration_parameters,
                            origin_project_declaration_variants,
                        )
                    )
                }
                lily::SyntaxDeclaration::TypeAlias {
                    type_keyword_range: _,
                    name: maybe_declaration_name,
                    parameters: origin_project_declaration_parameters,
                    equals_key_symbol_range: _,
                    type_,
                } => present_type_alias_declaration_info_markdown(
                    maybe_declaration_name.as_ref().map(|n| &n.value),
                    documentation,
                    origin_project_declaration_parameters,
                    type_.as_ref().map(lily::syntax_node_as_ref),
                ),
                lily::SyntaxDeclaration::Variable {
                    name: variable_name,
                    result: _,
                } => present_variable_declaration_info_with_complete_type_markdown(
                    documentation,
                    hovered_project_state
                        .variable_declarations
                        .get(&variable_name.value)
                        .and_then(|info| info.type_.as_ref()),
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
        LilySyntaxSymbol::LocalVariableDeclarationName {
            name: _,
            type_: maybe_type_type,
            scope_expression: _,
        } => Some(lsp_types::Hover {
            contents: lsp_types::HoverContents::Markup(lsp_types::MarkupContent {
                kind: lsp_types::MarkupKind::Markdown,
                value: local_variable_declaration_info_markdown(maybe_type_type.as_ref()),
            }),
            range: Some(hovered_symbol_node.range),
        }),
        LilySyntaxSymbol::Variable {
            name: hovered_name,
            local_bindings,
        } => {
            if let Some(hovered_local_binding_info) = local_bindings.get(hovered_name.as_str()) {
                return Some(lsp_types::Hover {
                    contents: lsp_types::HoverContents::Markup(lsp_types::MarkupContent {
                        kind: lsp_types::MarkupKind::Markdown,
                        value: local_binding_info_markdown(
                            hovered_local_binding_info.type_.as_ref(),
                            hovered_local_binding_info.origin,
                        ),
                    }),
                    range: Some(hovered_symbol_node.range),
                });
            }
            let origin_compiled_variable_declaration_info = hovered_project_state
                .variable_declarations
                .get(hovered_name)?;
            let origin_declaration_info_markdown: String =
                present_variable_declaration_info_with_complete_type_markdown(
                    origin_compiled_variable_declaration_info
                        .documentation
                        .as_deref(),
                    origin_compiled_variable_declaration_info.type_.as_ref(),
                );
            Some(lsp_types::Hover {
                contents: lsp_types::HoverContents::Markup(lsp_types::MarkupContent {
                    kind: lsp_types::MarkupKind::Markdown,
                    value: origin_declaration_info_markdown,
                }),
                range: Some(hovered_symbol_node.range),
            })
        }
        LilySyntaxSymbol::Field {
            name: _,
            value_type: maybe_value_type,
            fields_sorted,
        } => Some(lsp_types::Hover {
            contents: lsp_types::HoverContents::Markup(lsp_types::MarkupContent {
                kind: lsp_types::MarkupKind::Markdown,
                value: field_info_markdown(maybe_value_type.as_ref(), &fields_sorted),
            }),
            range: Some(hovered_symbol_node.range),
        }),
        LilySyntaxSymbol::InRecord { fields_sorted: _ } => None,
        LilySyntaxSymbol::Variant {
            name: hovered_name,
            type_: maybe_type,
        } => {
            let (
                origin_project_choice_type_declaration_name,
                origin_project_choice_type_declaration,
            ): (lily::Name, &lily::ChoiceTypeInfo) = maybe_type
                .and_then(|type_| {
                    lily_syntax_type_to_choice_type(
                        &hovered_project_state.type_aliases,
                        lily::syntax_node_empty(type_),
                    )
                    .and_then(|(origin_choice_type_name, _)| {
                        hovered_project_state
                            .choice_types
                            .get(&origin_choice_type_name)
                            .map(|origin_choice_type| (origin_choice_type_name, origin_choice_type))
                    })
                })
                .or_else(|| {
                    hovered_project_state.choice_types.iter().find_map(
                        |(
                            origin_project_choice_type_declaration_name,
                            origin_project_choice_type_declaration,
                        )| {
                            let any_declared_name_matches_hovered: bool =
                                origin_project_choice_type_declaration.variants.iter().any(
                                    |variant| {
                                        variant.name.as_ref().is_some_and(|name_node| {
                                            &name_node.value == hovered_name
                                        })
                                    },
                                );
                            if !any_declared_name_matches_hovered {
                                None
                            } else {
                                Some((
                                    origin_project_choice_type_declaration_name.clone(),
                                    origin_project_choice_type_declaration,
                                ))
                            }
                        },
                    )
                })?;
            Some(lsp_types::Hover {
                contents: lsp_types::HoverContents::Markup(lsp_types::MarkupContent {
                    kind: lsp_types::MarkupKind::Markdown,
                    value: format!(
                        "variant in\n{}",
                        &present_choice_type_declaration_info_markdown(
                            Some(&origin_project_choice_type_declaration_name),
                            origin_project_choice_type_declaration
                                .documentation
                                .as_deref(),
                            &origin_project_choice_type_declaration.parameters,
                            &origin_project_choice_type_declaration.variants,
                        )
                    ),
                }),
                range: Some(hovered_symbol_node.range),
            })
        }
        LilySyntaxSymbol::Type(hovered_name) => {
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
                        .map(lily::syntax_node_as_ref),
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
    maybe_type: Option<&lily::Type>,
    origin: LocalBindingOrigin,
) -> String {
    match origin {
        LocalBindingOrigin::PatternVariable(_) => match maybe_type {
            None => "variable introduced in pattern".to_string(),
            Some(type_) => {
                let mut type_string: String = String::new();
                lily::type_info_into(&mut type_string, 1, type_);
                format!(
                    "variable introduced in pattern
```lily
:{}{}:
```
",
                    type_string,
                    if type_string.contains('\n') { "\n" } else { "" }
                )
            }
        },
        LocalBindingOrigin::LocalDeclaredVariable { name_range: _ } => {
            local_variable_declaration_info_markdown(maybe_type)
        }
    }
}
fn field_info_markdown(maybe_type: Option<&lily::Type>, fields_sorted: &[lily::Name]) -> String {
    match maybe_type {
        None => format!(
            "record field. existing fields are: {}",
            fields_sorted.join(", ")
        ),
        Some(type_) => {
            let mut type_string: String = String::new();
            lily::type_info_into(&mut type_string, 1, type_);
            format!(
                "record field
```lily
:{}{}:
```
existing fields are: {}
",
                type_string,
                if type_string.contains('\n') { "\n" } else { "" },
                fields_sorted.join(", ")
            )
        }
    }
}
fn local_variable_declaration_info_markdown(maybe_type_type: Option<&lily::Type>) -> String {
    match maybe_type_type {
        None => "let variable".to_string(),
        Some(hovered_local_binding_type) => {
            let mut type_string: String = String::new();
            lily::type_info_into(&mut type_string, 1, hovered_local_binding_type);
            format!(
                "local variable
```lily
:{}{}:
```
",
                type_string,
                if type_string.contains('\n') { "\n" } else { "" }
            )
        }
    }
}

fn respond_to_goto_definition(
    state: &State,
    goto_definition_arguments: lsp_types::GotoDefinitionParams,
) -> Option<lsp_types::GotoDefinitionResponse> {
    let goto_symbol_project_state = state.projects.get(
        &goto_definition_arguments
            .text_document_position_params
            .text_document
            .uri,
    )?;
    let goto_symbol_node: lily::SyntaxNode<LilySyntaxSymbol> =
        lily_syntax_project_find_symbol_at_position(
            &goto_symbol_project_state.syntax,
            &goto_symbol_project_state.type_aliases,
            &goto_symbol_project_state.choice_types,
            &goto_symbol_project_state.variable_declarations,
            goto_definition_arguments
                .text_document_position_params
                .position,
        )?;
    match goto_symbol_node.value {
        LilySyntaxSymbol::LocalVariableDeclarationName { .. }
        | LilySyntaxSymbol::ProjectDeclarationName { .. }
        | LilySyntaxSymbol::Field { .. } => {
            // already at definition
            None
        }
        LilySyntaxSymbol::InRecord { fields_sorted: _ } => None,
        LilySyntaxSymbol::TypeVariable {
            scope_declaration,
            name: goto_type_variable_name,
        } => {
            match scope_declaration {
                lily::SyntaxDeclaration::ChoiceType {
                    name: _,
                    parameters: origin_type_parameters,
                    variants: _,
                } => {
                    let goto_type_variable_name_origin_parameter_node = origin_type_parameters
                        .iter()
                        .find(|origin_choice_type_parameter| {
                            &origin_choice_type_parameter.value == goto_type_variable_name
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
                lily::SyntaxDeclaration::TypeAlias {
                    type_keyword_range: _,
                    name: _,
                    parameters: origin_type_parameters,
                    equals_key_symbol_range: _,
                    type_: _,
                } => {
                    let goto_type_variable_name_origin_parameter_node = origin_type_parameters
                        .iter()
                        .find(|origin_choice_type_parameter| {
                            &origin_choice_type_parameter.value == goto_type_variable_name
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
                lily::SyntaxDeclaration::Variable { .. } => None,
            }
        }
        LilySyntaxSymbol::Variable {
            name: goto_name,
            local_bindings,
        } => {
            if let Some(goto_local_binding_info) = local_bindings.get(goto_name.as_str()) {
                return Some(lsp_types::GotoDefinitionResponse::Scalar(
                    lsp_types::Location {
                        uri: goto_definition_arguments
                            .text_document_position_params
                            .text_document
                            .uri,
                        range: match goto_local_binding_info.origin {
                            LocalBindingOrigin::PatternVariable(range) => range,
                            LocalBindingOrigin::LocalDeclaredVariable { name_range } => name_range,
                        },
                    },
                ));
            }
            let declaration_name_range: lsp_types::Range = goto_symbol_project_state
                .syntax
                .declarations
                .iter()
                .find_map(|origin_project_declaration_or_err| {
                    let Ok(lily::SyntaxDocumentedDeclaration {
                        documentation: _,
                        declaration: Some(declaration_node),
                    }) = origin_project_declaration_or_err
                    else {
                        return None;
                    };
                    let lily::SyntaxDeclaration::Variable {
                        name: name_node,
                        result: _,
                    } = &declaration_node.value
                    else {
                        return None;
                    };
                    if &name_node.value == goto_name {
                        Some(name_node.range)
                    } else {
                        None
                    }
                })?;
            Some(lsp_types::GotoDefinitionResponse::Scalar(
                lsp_types::Location {
                    uri: goto_definition_arguments
                        .text_document_position_params
                        .text_document
                        .uri,
                    range: declaration_name_range,
                },
            ))
        }
        LilySyntaxSymbol::Variant {
            name: goto_name,
            type_: maybe_type,
        } => {
            let origin_choice_type_variant_name_range: lsp_types::Range = maybe_type
                .and_then(|type_| {
                    lily_syntax_type_to_choice_type(
                        &goto_symbol_project_state.type_aliases,
                        lily::syntax_node_empty(type_),
                    )
                    .and_then(|(origin_choice_type_name, _)| {
                        goto_symbol_project_state
                            .choice_types
                            .get(&origin_choice_type_name)
                            .and_then(|origin_choice_type| {
                                origin_choice_type.variants.iter().find_map(
                                    |origin_choice_type_variant| {
                                        origin_choice_type_variant.name.as_ref().and_then(
                                            |origin_choice_type_variant_name_node| {
                                                if &origin_choice_type_variant_name_node.value
                                                    == goto_name
                                                {
                                                    Some(origin_choice_type_variant_name_node.range)
                                                } else {
                                                    None
                                                }
                                            },
                                        )
                                    },
                                )
                            })
                    })
                })
                .or_else(|| {
                    goto_symbol_project_state.choice_types.values().find_map(
                        |origin_project_choice_type| {
                            origin_project_choice_type
                                .variants
                                .iter()
                                .find_map(|variant| {
                                    variant.name.as_ref().and_then(|variant_name_node| {
                                        if &variant_name_node.value == goto_name {
                                            Some(variant_name_node.range)
                                        } else {
                                            None
                                        }
                                    })
                                })
                        },
                    )
                })?;
            Some(lsp_types::GotoDefinitionResponse::Scalar(
                lsp_types::Location {
                    uri: goto_definition_arguments
                        .text_document_position_params
                        .text_document
                        .uri,
                    range: origin_choice_type_variant_name_range,
                },
            ))
        }
        LilySyntaxSymbol::Type(goto_name) => {
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
                        .uri,
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
    let project_state = state
        .projects
        .get(&prepare_rename_arguments.text_document.uri)?;
    let prepare_rename_symbol_node: lily::SyntaxNode<LilySyntaxSymbol> =
        lily_syntax_project_find_symbol_at_position(
            &project_state.syntax,
            &project_state.type_aliases,
            &project_state.choice_types,
            &project_state.variable_declarations,
            prepare_rename_arguments.position,
        )?;
    match prepare_rename_symbol_node.value {
        LilySyntaxSymbol::Field {
            name,
            value_type: _,
            fields_sorted: _,
        }
        | LilySyntaxSymbol::ProjectDeclarationName {
            name,
            declaration: _,
            documentation: _,
        }
        | LilySyntaxSymbol::LocalVariableDeclarationName {
            name,
            type_: _,
            scope_expression: _,
        }
        | LilySyntaxSymbol::TypeVariable {
            scope_declaration: _,
            name,
        }
        | LilySyntaxSymbol::Variable {
            name,
            local_bindings: _,
        }
        | LilySyntaxSymbol::Variant { name, type_: _ }
        | LilySyntaxSymbol::Type(name) => {
            Some(Ok(lsp_types::PrepareRenameResponse::RangeWithPlaceholder {
                range: prepare_rename_symbol_node.range,
                placeholder: name.to_string(),
            }))
        }
        LilySyntaxSymbol::InRecord { fields_sorted: _ } => None,
    }
}

fn respond_to_rename(
    state: &State,
    rename_arguments: lsp_types::RenameParams,
) -> Option<Vec<lsp_types::TextDocumentEdit>> {
    let to_prepare_for_rename_project_state = state
        .projects
        .get(&rename_arguments.text_document_position.text_document.uri)?;
    let symbol_to_rename_node: lily::SyntaxNode<LilySyntaxSymbol> =
        lily_syntax_project_find_symbol_at_position(
            &to_prepare_for_rename_project_state.syntax,
            &to_prepare_for_rename_project_state.type_aliases,
            &to_prepare_for_rename_project_state.choice_types,
            &to_prepare_for_rename_project_state.variable_declarations,
            rename_arguments.text_document_position.position,
        )?;
    match symbol_to_rename_node.value {
        LilySyntaxSymbol::TypeVariable {
            scope_declaration,
            name: type_variable_to_rename,
        } => {
            let mut all_uses_of_renamed_type_variable: Vec<lsp_types::Range> =
                Vec::with_capacity(2);
            lily_syntax_declaration_uses_of_symbol_into(
                &mut all_uses_of_renamed_type_variable,
                &to_prepare_for_rename_project_state.type_aliases,
                scope_declaration,
                LilySymbolToReference::TypeVariable(type_variable_to_rename),
            );
            Some(vec![lsp_types::TextDocumentEdit {
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
            }])
        }
        LilySyntaxSymbol::Field {
            name: to_rename_field_name,
            value_type: _,
            fields_sorted: to_rename_fields_sorted,
        } => {
            let lily_declared_symbol_to_rename: LilySymbolToReference =
                LilySymbolToReference::Field {
                    name: to_rename_field_name,
                    fields_sorted: &to_rename_fields_sorted,
                };
            let mut all_uses_of_project_member: Vec<lsp_types::Range> = Vec::with_capacity(4);
            lily_syntax_project_uses_of_symbol_into(
                &mut all_uses_of_project_member,
                &to_prepare_for_rename_project_state.type_aliases,
                &to_prepare_for_rename_project_state.syntax,
                lily_declared_symbol_to_rename,
            );
            Some(vec![lsp_types::TextDocumentEdit {
                text_document: lsp_types::OptionalVersionedTextDocumentIdentifier {
                    uri: rename_arguments.text_document_position.text_document.uri,
                    version: None,
                },
                edits: all_uses_of_project_member
                    .into_iter()
                    .map(|use_range_of_renamed_project| {
                        lsp_types::OneOf::Left(lsp_types::TextEdit {
                            range: use_range_of_renamed_project,
                            new_text: rename_arguments.new_name.clone(),
                        })
                    })
                    .collect::<Vec<_>>(),
            }])
        }
        LilySyntaxSymbol::InRecord { fields_sorted: _ } => None,
        LilySyntaxSymbol::ProjectDeclarationName {
            name: to_rename_declaration_name,
            documentation: _,
            declaration: declaration_node,
        } => {
            let lily_declared_symbol_to_rename: LilySymbolToReference = match declaration_node.value
            {
                lily::SyntaxDeclaration::Variable { .. } => LilySymbolToReference::Variable {
                    name: to_rename_declaration_name,
                    including_declaration_name: true,
                },
                lily::SyntaxDeclaration::TypeAlias { .. } => LilySymbolToReference::Type {
                    name: to_rename_declaration_name,
                    including_declaration_name: true,
                },
                lily::SyntaxDeclaration::ChoiceType {
                    name: origin_project_declaration_maybe_name,
                    ..
                } => {
                    if origin_project_declaration_maybe_name
                        .as_ref()
                        .is_some_and(|node| &node.value == to_rename_declaration_name)
                    {
                        LilySymbolToReference::Type {
                            name: to_rename_declaration_name,
                            including_declaration_name: true,
                        }
                    } else {
                        LilySymbolToReference::Variant {
                            origin_type_name: origin_project_declaration_maybe_name
                                .as_ref()
                                .map(|node| &node.value),
                            name: to_rename_declaration_name,
                            including_declaration_name: true,
                        }
                    }
                }
            };
            let mut all_uses_of_project_member: Vec<lsp_types::Range> = Vec::with_capacity(4);
            lily_syntax_project_uses_of_symbol_into(
                &mut all_uses_of_project_member,
                &to_prepare_for_rename_project_state.type_aliases,
                &to_prepare_for_rename_project_state.syntax,
                lily_declared_symbol_to_rename,
            );
            Some(vec![lsp_types::TextDocumentEdit {
                text_document: lsp_types::OptionalVersionedTextDocumentIdentifier {
                    uri: rename_arguments.text_document_position.text_document.uri,
                    version: None,
                },
                edits: all_uses_of_project_member
                    .into_iter()
                    .map(|use_range_of_renamed_project| {
                        lsp_types::OneOf::Left(lsp_types::TextEdit {
                            range: use_range_of_renamed_project,
                            new_text: rename_arguments.new_name.clone(),
                        })
                    })
                    .collect::<Vec<_>>(),
            }])
        }
        LilySyntaxSymbol::LocalVariableDeclarationName {
            name: to_rename_name,
            type_: _,
            scope_expression: maybe_scope_expression,
        } => {
            let mut all_uses_of_local_variable_declaration_to_rename: Vec<lsp_types::Range> =
                Vec::with_capacity(1);
            if let Some(scope_expression) = maybe_scope_expression {
                lily_syntax_expression_uses_of_symbol_into(
                    &mut all_uses_of_local_variable_declaration_to_rename,
                    &to_prepare_for_rename_project_state.type_aliases,
                    &[],
                    scope_expression,
                    LilySymbolToReference::LocalBinding {
                        name: to_rename_name,
                        including_local_declaration_name: true,
                    },
                );
            }
            Some(vec![lsp_types::TextDocumentEdit {
                text_document: lsp_types::OptionalVersionedTextDocumentIdentifier {
                    uri: rename_arguments.text_document_position.text_document.uri,
                    version: None,
                },
                edits: std::iter::once(symbol_to_rename_node.range)
                    .chain(all_uses_of_local_variable_declaration_to_rename.into_iter())
                    .map(|use_range_of_renamed_project| {
                        lsp_types::OneOf::Left(lsp_types::TextEdit {
                            range: use_range_of_renamed_project,
                            new_text: rename_arguments.new_name.clone(),
                        })
                    })
                    .collect::<Vec<_>>(),
            }])
        }
        LilySyntaxSymbol::Variable {
            name: to_rename_name,
            local_bindings,
        } => {
            if let Some(to_rename_local_binding_info) = local_bindings.get(to_rename_name.as_str())
            {
                let mut all_uses_of_local_binding_to_rename: Vec<lsp_types::Range> =
                    Vec::with_capacity(2);
                if let Some(scope_expression) = to_rename_local_binding_info.scope_expression {
                    lily_syntax_expression_uses_of_symbol_into(
                        &mut all_uses_of_local_binding_to_rename,
                        &to_prepare_for_rename_project_state.type_aliases,
                        &[],
                        scope_expression,
                        LilySymbolToReference::LocalBinding {
                            name: to_rename_name,
                            including_local_declaration_name: true,
                        },
                    );
                }
                Some(vec![lsp_types::TextDocumentEdit {
                    text_document: lsp_types::OptionalVersionedTextDocumentIdentifier {
                        uri: rename_arguments.text_document_position.text_document.uri,
                        version: None,
                    },
                    edits: std::iter::once(match to_rename_local_binding_info.origin {
                        LocalBindingOrigin::PatternVariable(range) => range,
                        LocalBindingOrigin::LocalDeclaredVariable { name_range } => name_range,
                    })
                    .chain(all_uses_of_local_binding_to_rename.into_iter())
                    .map(|use_range_of_renamed_project| {
                        lsp_types::OneOf::Left(lsp_types::TextEdit {
                            range: use_range_of_renamed_project,
                            new_text: rename_arguments.new_name.clone(),
                        })
                    })
                    .collect::<Vec<_>>(),
                }])
            } else {
                let symbol_to_find: LilySymbolToReference = LilySymbolToReference::Variable {
                    name: to_rename_name,
                    including_declaration_name: true,
                };
                let mut all_uses_of_renamed_variable: Vec<lsp_types::Range> = Vec::with_capacity(4);
                lily_syntax_project_uses_of_symbol_into(
                    &mut all_uses_of_renamed_variable,
                    &to_prepare_for_rename_project_state.type_aliases,
                    &to_prepare_for_rename_project_state.syntax,
                    symbol_to_find,
                );
                Some(vec![lsp_types::TextDocumentEdit {
                    text_document: lsp_types::OptionalVersionedTextDocumentIdentifier {
                        uri: rename_arguments.text_document_position.text_document.uri,
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
                }])
            }
        }
        LilySyntaxSymbol::Variant {
            name: to_rename_name,
            type_: maybe_type,
        } => {
            let maybe_origin_choice_type_name: Option<lily::Name> = maybe_type.and_then(|type_| {
                lily_syntax_type_to_choice_type(
                    &to_prepare_for_rename_project_state.type_aliases,
                    lily::syntax_node_empty(type_),
                )
                .map(|(name, _)| name)
            });
            let symbol_to_find: LilySymbolToReference = LilySymbolToReference::Variant {
                name: to_rename_name,
                including_declaration_name: true,
                origin_type_name: maybe_origin_choice_type_name.as_ref(),
            };
            let mut all_uses_of_renamed_variable: Vec<lsp_types::Range> = Vec::with_capacity(4);
            lily_syntax_project_uses_of_symbol_into(
                &mut all_uses_of_renamed_variable,
                &to_prepare_for_rename_project_state.type_aliases,
                &to_prepare_for_rename_project_state.syntax,
                symbol_to_find,
            );
            Some(vec![lsp_types::TextDocumentEdit {
                text_document: lsp_types::OptionalVersionedTextDocumentIdentifier {
                    uri: rename_arguments.text_document_position.text_document.uri,
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
            }])
        }
        LilySyntaxSymbol::Type(type_name_to_rename) => {
            let lily_declared_symbol_to_rename: LilySymbolToReference =
                LilySymbolToReference::Type {
                    name: type_name_to_rename,
                    including_declaration_name: true,
                };

            let mut all_uses_of_renamed_type: Vec<lsp_types::Range> = Vec::with_capacity(4);
            lily_syntax_project_uses_of_symbol_into(
                &mut all_uses_of_renamed_type,
                &to_prepare_for_rename_project_state.type_aliases,
                &to_prepare_for_rename_project_state.syntax,
                lily_declared_symbol_to_rename,
            );
            Some(vec![lsp_types::TextDocumentEdit {
                text_document: lsp_types::OptionalVersionedTextDocumentIdentifier {
                    uri: rename_arguments.text_document_position.text_document.uri,
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
            }])
        }
    }
}

fn respond_to_references(
    state: &State,
    references_arguments: &lsp_types::ReferenceParams,
) -> Option<Vec<lsp_types::Location>> {
    let to_find_project_state = state.projects.get(
        &references_arguments
            .text_document_position
            .text_document
            .uri,
    )?;
    let symbol_to_find_node: lily::SyntaxNode<LilySyntaxSymbol> =
        lily_syntax_project_find_symbol_at_position(
            &to_find_project_state.syntax,
            &to_find_project_state.type_aliases,
            &to_find_project_state.choice_types,
            &to_find_project_state.variable_declarations,
            references_arguments.text_document_position.position,
        )?;
    match symbol_to_find_node.value {
        LilySyntaxSymbol::TypeVariable {
            scope_declaration,
            name: type_variable_to_find,
        } => {
            let mut all_uses_of_found_type_variable: Vec<lsp_types::Range> = Vec::with_capacity(2);
            lily_syntax_declaration_uses_of_symbol_into(
                &mut all_uses_of_found_type_variable,
                &to_find_project_state.type_aliases,
                scope_declaration,
                LilySymbolToReference::TypeVariable(type_variable_to_find),
            );
            Some(
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
                    .collect::<Vec<_>>(),
            )
        }
        LilySyntaxSymbol::Field {
            name: to_find_field_name,
            value_type: _,
            fields_sorted: to_find_fields_sorted,
        } => {
            let lily_declared_symbol_to_find: LilySymbolToReference =
                LilySymbolToReference::Field {
                    name: to_find_field_name,
                    fields_sorted: &to_find_fields_sorted,
                };
            let mut all_uses_of_found_project_member: Vec<lsp_types::Range> = Vec::with_capacity(4);
            lily_syntax_project_uses_of_symbol_into(
                &mut all_uses_of_found_project_member,
                &to_find_project_state.type_aliases,
                &to_find_project_state.syntax,
                lily_declared_symbol_to_find,
            );
            Some(
                all_uses_of_found_project_member
                    .into_iter()
                    .map(|use_range_of_found_project| lsp_types::Location {
                        uri: references_arguments
                            .text_document_position
                            .text_document
                            .uri
                            .clone(),
                        range: use_range_of_found_project,
                    })
                    .collect::<Vec<_>>(),
            )
        }
        LilySyntaxSymbol::InRecord { fields_sorted: _ } => None,
        LilySyntaxSymbol::ProjectDeclarationName {
            name: to_find_name,
            documentation: _,
            declaration: _,
        } => {
            let lily_declared_symbol_to_find: LilySymbolToReference = if to_find_name
                .starts_with(|c: char| c.is_ascii_uppercase())
            {
                LilySymbolToReference::Type {
                    name: to_find_name,
                    including_declaration_name: references_arguments.context.include_declaration,
                }
            } else {
                LilySymbolToReference::Variable {
                    name: to_find_name,
                    including_declaration_name: references_arguments.context.include_declaration,
                }
            };
            let mut all_uses_of_found_project_member: Vec<lsp_types::Range> = Vec::with_capacity(4);
            lily_syntax_project_uses_of_symbol_into(
                &mut all_uses_of_found_project_member,
                &to_find_project_state.type_aliases,
                &to_find_project_state.syntax,
                lily_declared_symbol_to_find,
            );
            Some(
                all_uses_of_found_project_member
                    .into_iter()
                    .map(|use_range_of_found_project| lsp_types::Location {
                        uri: references_arguments
                            .text_document_position
                            .text_document
                            .uri
                            .clone(),
                        range: use_range_of_found_project,
                    })
                    .collect::<Vec<_>>(),
            )
        }
        LilySyntaxSymbol::LocalVariableDeclarationName {
            name: to_find_name,
            type_: _,
            scope_expression: maybe_scope_expression,
        } => {
            let mut all_uses_of_found_local_variable_declaration: Vec<lsp_types::Range> =
                Vec::with_capacity(2);
            if references_arguments.context.include_declaration {
                all_uses_of_found_local_variable_declaration.push(symbol_to_find_node.range);
            }
            if let Some(scope_expression) = maybe_scope_expression {
                lily_syntax_expression_uses_of_symbol_into(
                    &mut all_uses_of_found_local_variable_declaration,
                    &to_find_project_state.type_aliases,
                    &[],
                    scope_expression,
                    LilySymbolToReference::LocalBinding {
                        name: to_find_name,
                        including_local_declaration_name: references_arguments
                            .context
                            .include_declaration,
                    },
                );
            }
            Some(
                all_uses_of_found_local_variable_declaration
                    .into_iter()
                    .map(|use_range_of_found_project| lsp_types::Location {
                        uri: references_arguments
                            .text_document_position
                            .text_document
                            .uri
                            .clone(),
                        range: use_range_of_found_project,
                    })
                    .collect::<Vec<_>>(),
            )
        }
        LilySyntaxSymbol::Variable {
            name: to_find_name,
            local_bindings,
        } => {
            if let Some(to_find_local_binding_info) = local_bindings.get(to_find_name.as_str()) {
                let mut all_uses_of_found_local_binding: Vec<lsp_types::Range> =
                    Vec::with_capacity(2);
                if references_arguments.context.include_declaration {
                    all_uses_of_found_local_binding.push(match to_find_local_binding_info.origin {
                        LocalBindingOrigin::PatternVariable(range) => range,
                        LocalBindingOrigin::LocalDeclaredVariable { name_range } => name_range,
                    });
                }
                if let Some(scope_expression) = to_find_local_binding_info.scope_expression {
                    lily_syntax_expression_uses_of_symbol_into(
                        &mut all_uses_of_found_local_binding,
                        &to_find_project_state.type_aliases,
                        &[],
                        scope_expression,
                        LilySymbolToReference::LocalBinding {
                            name: to_find_name,
                            including_local_declaration_name: references_arguments
                                .context
                                .include_declaration,
                        },
                    );
                }
                Some(
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
                        .collect::<Vec<_>>(),
                )
            } else {
                let symbol_to_find: LilySymbolToReference = LilySymbolToReference::Variable {
                    name: to_find_name,
                    including_declaration_name: references_arguments.context.include_declaration,
                };
                let mut all_uses_of_found_variable: Vec<lsp_types::Range> = Vec::with_capacity(4);
                lily_syntax_project_uses_of_symbol_into(
                    &mut all_uses_of_found_variable,
                    &to_find_project_state.type_aliases,
                    &to_find_project_state.syntax,
                    symbol_to_find,
                );
                Some(
                    all_uses_of_found_variable
                        .into_iter()
                        .map(|use_range_of_found_project| lsp_types::Location {
                            uri: references_arguments
                                .text_document_position
                                .text_document
                                .uri
                                .clone(),
                            range: use_range_of_found_project,
                        })
                        .collect::<Vec<_>>(),
                )
            }
        }
        LilySyntaxSymbol::Variant {
            name: to_find_name,
            type_: maybe_type,
        } => {
            let maybe_origin_choice_type_name: Option<lily::Name> = maybe_type.and_then(|type_| {
                lily_syntax_type_to_choice_type(
                    &to_find_project_state.type_aliases,
                    lily::syntax_node_empty(type_),
                )
                .map(|(name, _)| name)
            });
            let symbol_to_find: LilySymbolToReference = LilySymbolToReference::Variant {
                origin_type_name: maybe_origin_choice_type_name.as_ref(),
                name: to_find_name,
                including_declaration_name: references_arguments.context.include_declaration,
            };
            let mut all_uses_of_found_variable: Vec<lsp_types::Range> = Vec::with_capacity(4);
            lily_syntax_project_uses_of_symbol_into(
                &mut all_uses_of_found_variable,
                &to_find_project_state.type_aliases,
                &to_find_project_state.syntax,
                symbol_to_find,
            );
            Some(
                all_uses_of_found_variable
                    .into_iter()
                    .map(|use_range_of_found_project| lsp_types::Location {
                        uri: references_arguments
                            .text_document_position
                            .text_document
                            .uri
                            .clone(),
                        range: use_range_of_found_project,
                    })
                    .collect::<Vec<_>>(),
            )
        }
        LilySyntaxSymbol::Type(type_name_to_find) => {
            let lily_declared_symbol_to_find: LilySymbolToReference = LilySymbolToReference::Type {
                name: type_name_to_find,
                including_declaration_name: references_arguments.context.include_declaration,
            };
            let mut all_uses_of_found_type: Vec<lsp_types::Range> = Vec::with_capacity(4);
            lily_syntax_project_uses_of_symbol_into(
                &mut all_uses_of_found_type,
                &to_find_project_state.type_aliases,
                &to_find_project_state.syntax,
                lily_declared_symbol_to_find,
            );
            Some(
                all_uses_of_found_type
                    .into_iter()
                    .map(|use_range_of_found_project| lsp_types::Location {
                        uri: references_arguments
                            .text_document_position
                            .text_document
                            .uri
                            .clone(),
                        range: use_range_of_found_project,
                    })
                    .collect::<Vec<_>>(),
            )
        }
    }
}

fn respond_to_semantic_tokens_full(
    state: &State,
    semantic_tokens_arguments: &lsp_types::SemanticTokensParams,
) -> Option<lsp_types::SemanticTokensResult> {
    let project_state = state
        .projects
        .get(&semantic_tokens_arguments.text_document.uri)?;
    let mut highlighting: Vec<lily::SyntaxNode<lily::SyntaxHighlightKind>> =
        Vec::with_capacity(project_state.source.len() / 16);
    lily::syntax_highlight_project_into(&mut highlighting, &project_state.syntax);
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
                                        &lily_syntax_highlight_kind_to_lsp_semantic_token_type(
                                            segment.value,
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

fn present_variable_declaration_info_with_complete_type_markdown(
    maybe_documentation: Option<&str>,
    maybe_variable_type: Option<&lily::Type>,
) -> String {
    let description: String = match maybe_variable_type {
        Some(variable_type) => {
            let mut type_string: String = String::new();
            lily::type_info_into(&mut type_string, 1, variable_type);
            format!(
                "project variable
```lily
:{type_string}{}:
```
",
                if type_string.contains('\n') { "\n" } else { "" },
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
fn present_type_alias_declaration_info_markdown(
    maybe_name: Option<&lily::Name>,
    maybe_documentation: Option<&str>,
    parameters: &[lily::SyntaxNode<lily::Name>],
    maybe_type: Option<lily::SyntaxNode<&lily::SyntaxType>>,
) -> String {
    let mut declaration_as_string: String = String::new();
    lily::syntax_type_alias_declaration_into(
        &mut declaration_as_string,
        maybe_name,
        parameters,
        maybe_type,
    );
    let description = format!("```lily\n{}\n```\n", declaration_as_string);
    match maybe_documentation {
        None => description,
        Some(documentation) => {
            description + "---\n" + documentation_comment_to_markdown(documentation).as_str()
        }
    }
}
fn present_choice_type_declaration_info_markdown(
    maybe_name: Option<&lily::Name>,
    maybe_documentation: Option<&str>,
    parameters: &[lily::SyntaxNode<lily::Name>],
    variants: &[lily::SyntaxChoiceTypeVariant],
) -> String {
    let declaration_string: String = if variants.is_empty() {
        format!(
            "choice {}",
            maybe_name.map(lily::Name::as_str).unwrap_or("")
        )
    } else {
        let mut declaration_string: String = String::new();
        lily::syntax_choice_type_declaration_into(
            &mut declaration_string,
            maybe_name,
            parameters,
            variants,
        );
        declaration_string
    };
    let description: String = format!("```lily\n{}\n```\n", declaration_string);
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
    let completion_project = state.projects.get(
        &completion_arguments
            .text_document_position
            .text_document
            .uri,
    )?;
    let symbol_to_complete: lily::SyntaxNode<LilySyntaxSymbol> =
        lily_syntax_project_find_symbol_at_position(
            &completion_project.syntax,
            &completion_project.type_aliases,
            &completion_project.choice_types,
            &completion_project.variable_declarations,
            completion_arguments.text_document_position.position,
        )?;
    let maybe_completion_items: Option<Vec<lsp_types::CompletionItem>> =
        match symbol_to_complete.value {
            LilySyntaxSymbol::ProjectDeclarationName { .. } => None,
            LilySyntaxSymbol::LocalVariableDeclarationName { .. } => {
                // we could suggest existing local bindings^
                // but that seems more annoying than useful
                None
            }
            LilySyntaxSymbol::Field {
                name: field_name_to_complete,
                value_type: _,
                fields_sorted,
            } => Some(
                completion_project
                    .records
                    .iter()
                    .filter(|project_record_fields| {
                        fields_sorted.iter().all(|field_name| {
                            field_name == field_name_to_complete
                                || project_record_fields.contains(field_name)
                        })
                    })
                    .flatten()
                    .filter(|field_name| !fields_sorted.contains(field_name))
                    .map(|field_name| lsp_types::CompletionItem {
                        label: field_name.to_string(),
                        kind: Some(lsp_types::CompletionItemKind::PROPERTY),
                        documentation: None,
                        text_edit: Some(lsp_types::CompletionTextEdit::Edit(lsp_types::TextEdit {
                            range: symbol_to_complete.range,
                            new_text: field_name.to_string(),
                        })),
                        ..lsp_types::CompletionItem::default()
                    })
                    .collect(),
            ),
            LilySyntaxSymbol::InRecord { fields_sorted } => Some(
                completion_project
                    .records
                    .iter()
                    .filter(|project_record_fields| {
                        fields_sorted
                            .iter()
                            .all(|field_name| project_record_fields.contains(field_name))
                    })
                    .flatten()
                    .filter(|field_name| !fields_sorted.contains(field_name))
                    .map(|field_name| lsp_types::CompletionItem {
                        label: field_name.to_string(),
                        kind: Some(lsp_types::CompletionItemKind::PROPERTY),
                        documentation: None,
                        text_edit: Some(lsp_types::CompletionTextEdit::Edit(lsp_types::TextEdit {
                            range: symbol_to_complete.range,
                            new_text: field_name.to_string(),
                        })),
                        ..lsp_types::CompletionItem::default()
                    })
                    .collect(),
            ),
            LilySyntaxSymbol::Variable {
                name: _,
                local_bindings,
            } => {
                let mut completion_items: Vec<lsp_types::CompletionItem> = Vec::new();
                let local_binding_completions =
                    local_bindings
                        .iter()
                        .map(|(local_binding_name, local_binding_info)| {
                            lsp_types::CompletionItem {
                                label: local_binding_name.to_string(),
                                kind: Some(lsp_types::CompletionItemKind::VARIABLE),
                                documentation: Some(lsp_types::Documentation::MarkupContent(
                                    lsp_types::MarkupContent {
                                        kind: lsp_types::MarkupKind::Markdown,
                                        value: local_binding_info_markdown(
                                            local_binding_info.type_.as_ref(),
                                            local_binding_info.origin,
                                        ),
                                    },
                                )),
                                text_edit: Some(lsp_types::CompletionTextEdit::Edit(
                                    lsp_types::TextEdit {
                                        range: symbol_to_complete.range,
                                        new_text: local_binding_name.to_string(),
                                    },
                                )),
                                ..lsp_types::CompletionItem::default()
                            }
                        });
                completion_items.extend(local_binding_completions);
                project_variable_completions_into(
                    &mut completion_items,
                    &completion_project.variable_declarations,
                    symbol_to_complete.range,
                );
                Some(completion_items)
            }
            LilySyntaxSymbol::Variant {
                name: _,
                type_: maybe_type,
            } => {
                let mut completion_items: Vec<lsp_types::CompletionItem> = Vec::new();
                variant_completions_into(
                    &mut completion_items,
                    &completion_project.choice_types,
                    &completion_project.type_aliases,
                    symbol_to_complete.range,
                    maybe_type,
                );
                Some(completion_items)
            }
            LilySyntaxSymbol::Type(_) => {
                let mut completion_items: Vec<lsp_types::CompletionItem> = Vec::new();
                type_declaration_completions_into(
                    &completion_project.type_aliases,
                    &completion_project.choice_types,
                    &mut completion_items,
                    symbol_to_complete.range,
                );
                Some(completion_items)
            }
            LilySyntaxSymbol::TypeVariable { .. } => {
                // is this ever useful to add? lily tends to use single-letter names anyway most of the time
                // (or ones where the first letters don't match in the first place).
                // suggesting completions can get annoying and isn't free computationally so...
                None
            }
        };
    maybe_completion_items.map(lsp_types::CompletionResponse::Array)
}

fn project_variable_completions_into(
    completion_items: &mut Vec<lsp_types::CompletionItem>,
    variable_declarations: &std::collections::HashMap<
        lily::Name,
        lily::CompiledVariableDeclarationInfo,
    >,
    symbol_to_complete_range: lsp_types::Range,
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
            text_edit: Some(lsp_types::CompletionTextEdit::Edit(lsp_types::TextEdit {
                range: symbol_to_complete_range,
                new_text: variable_declaration_name.to_string(),
            })),
            ..lsp_types::CompletionItem::default()
        },
    ));
}
fn variant_completions_into(
    completion_items: &mut Vec<lsp_types::CompletionItem>,
    choice_types: &std::collections::HashMap<lily::Name, lily::ChoiceTypeInfo>,
    type_aliases: &std::collections::HashMap<lily::Name, lily::TypeAliasInfo>,
    symbol_to_complete_range: lsp_types::Range,
    maybe_type: Option<&lily::SyntaxType>,
) {
    let maybe_origin_choice_type: Option<(lily::Name, &lily::ChoiceTypeInfo)> = maybe_type
        .and_then(|type_| {
            lily_syntax_type_to_choice_type(type_aliases, lily::syntax_node_empty(type_)).and_then(
                |(origin_choice_type_name, _)| {
                    choice_types
                        .get(&origin_choice_type_name)
                        .map(|origin_choice_type| (origin_choice_type_name, origin_choice_type))
                },
            )
        });
    match maybe_origin_choice_type {
        Some((origin_choice_type_name, origin_choice_type)) => {
            let info_markdown: String = format!(
                "variant in\n{}",
                present_choice_type_declaration_info_markdown(
                    Some(&origin_choice_type_name),
                    origin_choice_type.documentation.as_deref(),
                    &origin_choice_type.parameters,
                    &origin_choice_type.variants,
                ),
            );
            completion_items.extend(
                origin_choice_type
                    .variants
                    .iter()
                    .filter_map(|variant| variant.name.as_ref().map(|node| node.value.to_string()))
                    .map(move |variant_name| lsp_types::CompletionItem {
                        label: variant_name.clone(),
                        kind: Some(lsp_types::CompletionItemKind::ENUM_MEMBER),
                        documentation: Some(lsp_types::Documentation::MarkupContent(
                            lsp_types::MarkupContent {
                                kind: lsp_types::MarkupKind::Markdown,
                                value: info_markdown.clone(),
                            },
                        )),
                        text_edit: Some(lsp_types::CompletionTextEdit::Edit(lsp_types::TextEdit {
                            range: symbol_to_complete_range,
                            new_text: variant_name,
                        })),
                        ..lsp_types::CompletionItem::default()
                    }),
            );
        }
        None => {
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
                        .filter_map(|variant| {
                            variant.name.as_ref().map(|node| node.value.to_string())
                        })
                        .map(move |variant_name| lsp_types::CompletionItem {
                            label: variant_name.clone(),
                            kind: Some(lsp_types::CompletionItemKind::ENUM_MEMBER),
                            documentation: Some(lsp_types::Documentation::MarkupContent(
                                lsp_types::MarkupContent {
                                    kind: lsp_types::MarkupKind::Markdown,
                                    value: info_markdown.clone(),
                                },
                            )),
                            text_edit: Some(lsp_types::CompletionTextEdit::Edit(
                                lsp_types::TextEdit {
                                    range: symbol_to_complete_range,
                                    new_text: variant_name,
                                },
                            )),
                            ..lsp_types::CompletionItem::default()
                        })
                },
            ));
        }
    }
}
fn type_declaration_completions_into(
    type_aliases: &std::collections::HashMap<lily::Name, lily::TypeAliasInfo>,
    choice_types: &std::collections::HashMap<lily::Name, lily::ChoiceTypeInfo>,
    completion_items: &mut Vec<lsp_types::CompletionItem>,
    symbol_to_complete_range: lsp_types::Range,
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
                text_edit: Some(lsp_types::CompletionTextEdit::Edit(lsp_types::TextEdit {
                    range: symbol_to_complete_range,
                    new_text: origin_project_choice_type_name.to_string(),
                })),
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
                                .map(lily::syntax_node_as_ref),
                        ),
                    },
                )),
                text_edit: Some(lsp_types::CompletionTextEdit::Edit(lsp_types::TextEdit {
                    range: symbol_to_complete_range,
                    new_text: type_alias_name.to_string(),
                })),
                ..lsp_types::CompletionItem::default()
            },
        ),
    );
}

fn respond_to_document_formatting(
    state: &State,
    formatting_arguments: &lsp_types::DocumentFormattingParams,
) -> Option<Vec<lsp_types::TextEdit>> {
    let to_format_project = state
        .projects
        .get(&formatting_arguments.text_document.uri)?;
    let formatted: String =
        lily::syntax_project_format(&to_format_project.syntax, &to_format_project.source);
    // diffing does not seem to be needed here. But maybe it's faster?
    Some(vec![lsp_types::TextEdit {
        range: lsp_types::Range {
            start: lsp_types::Position {
                line: 0,
                character: 0,
            },
            end: lsp_types::Position {
                line: to_format_project.source.lines().count() as u32
                    + (
                        // restore last line break potentially eaten by .lines()
                        if to_format_project.source.ends_with(['\r', '\n']) {
                            1
                        } else {
                            0
                        }
                    )
                    + 1,
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
    let project = state
        .projects
        .get(&document_symbol_arguments.text_document.uri)?;
    Some(lsp_types::DocumentSymbolResponse::Nested(
        project
            .syntax
            .declarations
            .iter()
            .filter_map(|declaration_or_err| declaration_or_err.as_ref().ok())
            .filter_map(|documented_declaration| documented_declaration.declaration.as_ref())
            .filter_map(|declaration_node| match &declaration_node.value {
                lily::SyntaxDeclaration::ChoiceType {
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
                lily::SyntaxDeclaration::TypeAlias {
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
                lily::SyntaxDeclaration::Variable {
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

fn lily_error_node_to_diagnostic(problem: &lily::ErrorNode) -> lsp_types::Diagnostic {
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
    markdown_convert_code_blocks_to_lily_into(&mut builder, markdown_source);
    builder
}
fn markdown_convert_code_blocks_to_lily_into(builder: &mut String, markdown_source: &str) {
    // because I don't want to introduce a full markdown parser for just this tiny
    // improvement, the code below only approximates where code blocks are.
    let mut with_fenced_code_blocks_converted: String = String::new();
    markdown_convert_unspecific_fenced_code_blocks_to_lily_into(
        &mut with_fenced_code_blocks_converted,
        markdown_source,
    );
    markdown_convert_indented_code_blocks_to_lily(builder, &with_fenced_code_blocks_converted);
}

/// replace fenced no-language-specified code blocks by `lily...`
fn markdown_convert_unspecific_fenced_code_blocks_to_lily_into(
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
                                result_builder.push_str("```lily");
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

fn markdown_convert_indented_code_blocks_to_lily(builder: &mut String, markdown_source: &str) {
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
                    builder.push_str("```lily\n");
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

#[derive(Copy, Clone)]
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
            "before line > after line (before: {}, after: {})",
            lsp_position_to_string(before),
            lsp_position_to_string(after)
        )),
        std::cmp::Ordering::Equal => {
            if before.character > after.character {
                Err(format!(
                    "before character > after character (before: {}, after: {})",
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
fn lily_syntax_highlight_kind_to_lsp_semantic_token_type(
    lily_syntax_highlight_kind: lily::SyntaxHighlightKind,
) -> lsp_types::SemanticTokenType {
    match lily_syntax_highlight_kind {
        lily::SyntaxHighlightKind::KeySymbol => lsp_types::SemanticTokenType::KEYWORD,
        lily::SyntaxHighlightKind::Field => lsp_types::SemanticTokenType::PROPERTY,
        lily::SyntaxHighlightKind::Type => lsp_types::SemanticTokenType::TYPE,
        lily::SyntaxHighlightKind::Variable => lsp_types::SemanticTokenType::VARIABLE,
        lily::SyntaxHighlightKind::Variant => lsp_types::SemanticTokenType::ENUM_MEMBER,
        lily::SyntaxHighlightKind::DeclaredVariable => lsp_types::SemanticTokenType::FUNCTION,
        lily::SyntaxHighlightKind::Comment => lsp_types::SemanticTokenType::COMMENT,
        lily::SyntaxHighlightKind::Number => lsp_types::SemanticTokenType::NUMBER,
        lily::SyntaxHighlightKind::String => lsp_types::SemanticTokenType::STRING,
        lily::SyntaxHighlightKind::TypeVariable => lsp_types::SemanticTokenType::TYPE_PARAMETER,
    }
}

#[derive(Clone, Debug)]
enum LilySyntaxSymbol<'a> {
    // includes variant
    ProjectDeclarationName {
        name: &'a lily::Name,
        documentation: Option<&'a str>,
        declaration: lily::SyntaxNode<&'a lily::SyntaxDeclaration>,
    },
    LocalVariableDeclarationName {
        name: &'a lily::Name,
        type_: Option<lily::Type>,
        scope_expression: Option<lily::SyntaxNode<&'a lily::SyntaxExpression>>,
    },
    Variable {
        name: &'a lily::Name,
        // consider wrapping in Option
        local_bindings: std::collections::HashMap<&'a str, LilyLocalBindingInfo<'a>>,
    },
    Variant {
        name: &'a lily::Name,
        type_: Option<&'a lily::SyntaxType>,
    },
    Type(&'a lily::Name),
    TypeVariable {
        scope_declaration: &'a lily::SyntaxDeclaration,
        name: &'a lily::Name,
    },
    Field {
        name: &'a lily::Name,
        value_type: Option<lily::Type>,
        fields_sorted: Vec<lily::Name>,
    },
    InRecord {
        fields_sorted: Vec<lily::Name>,
    },
}
#[derive(Clone, Debug)]
struct LilyLocalBindingInfo<'a> {
    type_: Option<lily::Type>,
    origin: LocalBindingOrigin,
    scope_expression: Option<lily::SyntaxNode<&'a lily::SyntaxExpression>>,
}

fn lily_syntax_project_find_symbol_at_position<'a>(
    syntax_project: &'a lily::SyntaxProject,
    type_aliases: &'a std::collections::HashMap<lily::Name, lily::TypeAliasInfo>,
    choice_types: &'a std::collections::HashMap<lily::Name, lily::ChoiceTypeInfo>,
    variable_declarations: &std::collections::HashMap<
        lily::Name,
        lily::CompiledVariableDeclarationInfo,
    >,
    position: lsp_types::Position,
) -> Option<lily::SyntaxNode<LilySyntaxSymbol<'a>>> {
    syntax_project
        .declarations
        .iter()
        .filter_map(|declaration_or_err| declaration_or_err.as_ref().ok())
        .find_map(|documented_declaration| {
            let declaration_node = documented_declaration.declaration.as_ref()?;
            lily_syntax_declaration_find_symbol_at_position(
                type_aliases,
                choice_types,
                variable_declarations,
                lily::syntax_node_as_ref(declaration_node),
                documented_declaration
                    .documentation
                    .as_ref()
                    .map(|node| node.value.as_ref()),
                position,
            )
        })
}

fn lily_syntax_declaration_find_symbol_at_position<'a>(
    type_aliases: &std::collections::HashMap<lily::Name, lily::TypeAliasInfo>,
    choice_types: &std::collections::HashMap<lily::Name, lily::ChoiceTypeInfo>,
    variable_declarations: &std::collections::HashMap<
        lily::Name,
        lily::CompiledVariableDeclarationInfo,
    >,
    declaration_node: lily::SyntaxNode<&'a lily::SyntaxDeclaration>,
    maybe_documentation: Option<&'a str>,
    position: lsp_types::Position,
) -> Option<lily::SyntaxNode<LilySyntaxSymbol<'a>>> {
    if !lsp_range_includes_position(declaration_node.range, position) {
        None
    } else {
        match declaration_node.value {
            lily::SyntaxDeclaration::ChoiceType {
                name: maybe_name,
                parameters,
                variants,
            } => {
                if let Some(name_node) = maybe_name
                    && lsp_range_includes_position(
                        lsp_types::Range {
                            start: declaration_node.range.start,
                            end: name_node.range.end,
                        },
                        position,
                    )
                {
                    return Some(lily::SyntaxNode {
                        value: LilySyntaxSymbol::ProjectDeclarationName {
                            name: &name_node.value,
                            declaration: declaration_node,
                            documentation: maybe_documentation,
                        },
                        range: name_node.range,
                    });
                }
                parameters
                    .iter()
                    .find_map(|parameter_node| {
                        if lsp_range_includes_position(parameter_node.range, position) {
                            Some(lily::SyntaxNode {
                                value: LilySyntaxSymbol::TypeVariable {
                                    scope_declaration: declaration_node.value,
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
                                && lsp_range_includes_position(variant_name_node.range, position)
                            {
                                Some(lily::SyntaxNode {
                                    value: LilySyntaxSymbol::ProjectDeclarationName {
                                        name: &variant_name_node.value,
                                        declaration: declaration_node,
                                        documentation: maybe_documentation,
                                    },
                                    range: variant_name_node.range,
                                })
                            } else {
                                variant.value.iter().find_map(|variant_value| {
                                    lily_syntax_type_find_symbol_at_position(
                                        type_aliases,
                                        choice_types,
                                        declaration_node.value,
                                        lily::syntax_node_as_ref(variant_value),
                                        position,
                                    )
                                })
                            }
                        })
                    })
            }
            lily::SyntaxDeclaration::TypeAlias {
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
                    return Some(lily::SyntaxNode {
                        value: LilySyntaxSymbol::ProjectDeclarationName {
                            name: &name_node.value,
                            declaration: declaration_node,
                            documentation: maybe_documentation,
                        },
                        range: name_node.range,
                    });
                }
                parameters
                    .iter()
                    .find_map(|parameter_node| {
                        if lsp_range_includes_position(parameter_node.range, position) {
                            Some(lily::SyntaxNode {
                                value: LilySyntaxSymbol::TypeVariable {
                                    scope_declaration: declaration_node.value,
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
                            lily_syntax_type_find_symbol_at_position(
                                type_aliases,
                                choice_types,
                                declaration_node.value,
                                lily::syntax_node_as_ref(type_node),
                                position,
                            )
                        })
                    })
            }
            lily::SyntaxDeclaration::Variable {
                name: name_node,
                result: maybe_result,
            } => {
                if lsp_range_includes_position(name_node.range, position) {
                    return Some(lily::SyntaxNode {
                        value: LilySyntaxSymbol::ProjectDeclarationName {
                            name: &name_node.value,
                            declaration: declaration_node,
                            documentation: maybe_documentation,
                        },
                        range: name_node.range,
                    });
                }
                maybe_result.as_ref().and_then(|result_node| {
                    lily_syntax_expression_find_symbol_at_position(
                        type_aliases,
                        choice_types,
                        variable_declarations,
                        declaration_node.value,
                        std::collections::HashMap::new(),
                        lily::syntax_node_as_ref(result_node),
                        position,
                    )
                    .break_value()
                })
            }
        }
    }
}

fn lily_syntax_pattern_find_symbol_at_position<'a>(
    type_aliases: &std::collections::HashMap<lily::Name, lily::TypeAliasInfo>,
    choice_types: &std::collections::HashMap<lily::Name, lily::ChoiceTypeInfo>,
    scope_declaration: &'a lily::SyntaxDeclaration,
    scope_expression: Option<lily::SyntaxNode<&'a lily::SyntaxExpression>>,
    pattern_node: lily::SyntaxNode<&'a lily::SyntaxPattern>,
    position: lsp_types::Position,
) -> Option<lily::SyntaxNode<LilySyntaxSymbol<'a>>> {
    if !lsp_range_includes_position(pattern_node.range, position) {
        return None;
    }
    match pattern_node.value {
        lily::SyntaxPattern::Unt { .. } => None,
        lily::SyntaxPattern::Int { .. } => None,
        lily::SyntaxPattern::Char(_) => None,
        lily::SyntaxPattern::String { .. } => None,
        lily::SyntaxPattern::Typed {
            type_: maybe_type_node,
            closing_colon_range: _,
            pattern: maybe_pattern_node_in_typed,
        } => maybe_type_node
            .as_ref()
            .and_then(|type_node| {
                lily_syntax_type_find_symbol_at_position(
                    type_aliases,
                    choice_types,
                    scope_declaration,
                    lily::syntax_node_as_ref(type_node),
                    position,
                )
            })
            .or_else(|| {
                let pattern_node_in_typed = maybe_pattern_node_in_typed.as_ref()?;
                match pattern_node_in_typed.value.as_ref() {
                    lily::SyntaxPattern::Variable {
                        overwriting: _,
                        name,
                    } => Some(lily::SyntaxNode {
                        range: pattern_node_in_typed.range,
                        value: LilySyntaxSymbol::Variable {
                            name: name,
                            local_bindings: std::collections::HashMap::from([(
                                name.as_str(),
                                LilyLocalBindingInfo {
                                    type_: maybe_type_node.as_ref().and_then(|type_node| {
                                        lily::syntax_type_to_type(
                                            &mut Vec::new(),
                                            type_aliases,
                                            choice_types,
                                            lily::syntax_node_as_ref(type_node),
                                        )
                                    }),
                                    origin: LocalBindingOrigin::PatternVariable(
                                        pattern_node_in_typed.range,
                                    ),
                                    scope_expression: scope_expression,
                                },
                            )]),
                        },
                    }),
                    lily::SyntaxPattern::Variant {
                        name: variable,
                        value: maybe_value,
                    } => {
                        if lsp_range_includes_position(variable.range, position) {
                            return Some(lily::SyntaxNode {
                                value: LilySyntaxSymbol::Variant {
                                    name: &variable.value,
                                    type_: maybe_type_node.as_ref().map(|n| &n.value),
                                },
                                range: variable.range,
                            });
                        }
                        maybe_value.as_ref().and_then(|value| {
                            lily_syntax_pattern_find_symbol_at_position(
                                type_aliases,
                                choice_types,
                                scope_declaration,
                                scope_expression,
                                lily::syntax_node_unbox(value),
                                position,
                            )
                        })
                    }
                    other_in_typed => lily_syntax_pattern_find_symbol_at_position(
                        type_aliases,
                        choice_types,
                        scope_declaration,
                        scope_expression,
                        lily::SyntaxNode {
                            range: pattern_node_in_typed.range,
                            value: other_in_typed,
                        },
                        position,
                    ),
                }
            }),
        lily::SyntaxPattern::Ignored => None,
        lily::SyntaxPattern::Variable {
            overwriting: _,
            name,
        } => Some(lily::SyntaxNode {
            range: pattern_node.range,
            value: LilySyntaxSymbol::Variable {
                name: name,
                local_bindings: std::collections::HashMap::from([(
                    name.as_str(),
                    LilyLocalBindingInfo {
                        type_: None,
                        origin: LocalBindingOrigin::PatternVariable(pattern_node.range),
                        scope_expression: scope_expression,
                    },
                )]),
            },
        }),
        lily::SyntaxPattern::Variant {
            name: variable,
            value: maybe_value,
        } => {
            if lsp_range_includes_position(variable.range, position) {
                return Some(lily::SyntaxNode {
                    value: LilySyntaxSymbol::Variant {
                        name: &variable.value,
                        type_: None,
                    },
                    range: variable.range,
                });
            }
            maybe_value.as_ref().and_then(|value| {
                lily_syntax_pattern_find_symbol_at_position(
                    type_aliases,
                    choice_types,
                    scope_declaration,
                    scope_expression,
                    lily::syntax_node_unbox(value),
                    position,
                )
            })
        }
        lily::SyntaxPattern::WithComment {
            comment: _,
            pattern: maybe_pattern_after_expression,
        } => maybe_pattern_after_expression
            .as_ref()
            .and_then(|pattern_node_after_expression| {
                lily_syntax_pattern_find_symbol_at_position(
                    type_aliases,
                    choice_types,
                    scope_declaration,
                    scope_expression,
                    lily::syntax_node_unbox(pattern_node_after_expression),
                    position,
                )
            }),
        lily::SyntaxPattern::Record(fields) => Some(
            fields
                .iter()
                .find_map(|field| {
                    if lsp_range_includes_position(field.name.range, position) {
                        return Some(lily::SyntaxNode {
                            value: LilySyntaxSymbol::Field {
                                name: &field.name.value,
                                value_type: field.value.as_ref().and_then(|field_value_node| {
                                    lily::syntax_pattern_type(
                                        type_aliases,
                                        choice_types,
                                        lily::syntax_node_as_ref(field_value_node),
                                    )
                                }),
                                fields_sorted: sorted_field_names(
                                    fields.iter().map(|record_field| &record_field.name.value),
                                ),
                            },
                            range: field.name.range,
                        });
                    }
                    field.value.as_ref().and_then(|field_value_node| {
                        lily_syntax_pattern_find_symbol_at_position(
                            type_aliases,
                            choice_types,
                            scope_declaration,
                            scope_expression,
                            lily::syntax_node_as_ref(field_value_node),
                            position,
                        )
                    })
                })
                .unwrap_or_else(|| lily::SyntaxNode {
                    range: lsp_types::Range {
                        start: position,
                        end: position,
                    },
                    value: LilySyntaxSymbol::InRecord {
                        fields_sorted: sorted_field_names(
                            fields.iter().map(|record_field| &record_field.name.value),
                        ),
                    },
                }),
        ),
    }
}

fn lily_syntax_type_find_symbol_at_position<'a>(
    type_aliases: &std::collections::HashMap<lily::Name, lily::TypeAliasInfo>,
    choice_types: &std::collections::HashMap<lily::Name, lily::ChoiceTypeInfo>,
    scope_declaration: &'a lily::SyntaxDeclaration,
    type_node: lily::SyntaxNode<&'a lily::SyntaxType>,
    position: lsp_types::Position,
) -> Option<lily::SyntaxNode<LilySyntaxSymbol<'a>>> {
    if !lsp_range_includes_position(type_node.range, position) {
        return None;
    }
    match type_node.value {
        lily::SyntaxType::Construct {
            name: variable,
            arguments,
        } => {
            if lsp_range_includes_position(variable.range, position) {
                return Some(lily::SyntaxNode {
                    value: LilySyntaxSymbol::Type(&variable.value),
                    range: variable.range,
                });
            }
            arguments.iter().find_map(|argument| {
                lily_syntax_type_find_symbol_at_position(
                    type_aliases,
                    choice_types,
                    scope_declaration,
                    lily::syntax_node_as_ref(argument),
                    position,
                )
            })
        }
        lily::SyntaxType::Function {
            inputs,
            arrow_key_symbol_range: _,
            output: maybe_output,
        } => inputs
            .iter()
            .find_map(|input_node| {
                lily_syntax_type_find_symbol_at_position(
                    type_aliases,
                    choice_types,
                    scope_declaration,
                    lily::syntax_node_as_ref(input_node),
                    position,
                )
            })
            .or_else(|| {
                maybe_output.as_ref().and_then(|output_node| {
                    lily_syntax_type_find_symbol_at_position(
                        type_aliases,
                        choice_types,
                        scope_declaration,
                        lily::syntax_node_unbox(output_node),
                        position,
                    )
                })
            }),
        lily::SyntaxType::Parenthesized(None) => None,
        lily::SyntaxType::Parenthesized(Some(in_parens)) => {
            lily_syntax_type_find_symbol_at_position(
                type_aliases,
                choice_types,
                scope_declaration,
                lily::syntax_node_unbox(in_parens),
                position,
            )
        }
        lily::SyntaxType::WithComment {
            comment: _,
            type_: maybe_type_after_comment,
        } => maybe_type_after_comment
            .as_ref()
            .and_then(|type_node_after_comment| {
                lily_syntax_type_find_symbol_at_position(
                    type_aliases,
                    choice_types,
                    scope_declaration,
                    lily::syntax_node_unbox(type_node_after_comment),
                    position,
                )
            }),
        lily::SyntaxType::Record(fields) => Some(
            fields
                .iter()
                .find_map(|field| {
                    if lsp_range_includes_position(field.name.range, position) {
                        return Some(lily::SyntaxNode {
                            value: LilySyntaxSymbol::Field {
                                name: &field.name.value,
                                value_type: field.value.as_ref().and_then(|field_value_node| {
                                    lily::syntax_type_to_type(
                                        &mut Vec::new(),
                                        type_aliases,
                                        choice_types,
                                        lily::syntax_node_as_ref(field_value_node),
                                    )
                                }),
                                fields_sorted: sorted_field_names(
                                    fields.iter().map(|record_field| &record_field.name.value),
                                ),
                            },
                            range: field.name.range,
                        });
                    }
                    field.value.as_ref().and_then(|field_value_node| {
                        lily_syntax_type_find_symbol_at_position(
                            type_aliases,
                            choice_types,
                            scope_declaration,
                            lily::syntax_node_as_ref(field_value_node),
                            position,
                        )
                    })
                })
                .unwrap_or_else(|| lily::SyntaxNode {
                    range: lsp_types::Range {
                        start: position,
                        end: position,
                    },
                    value: LilySyntaxSymbol::InRecord {
                        fields_sorted: sorted_field_names(
                            fields.iter().map(|record_field| &record_field.name.value),
                        ),
                    },
                }),
        ),
        lily::SyntaxType::Variable(type_variable_value) => Some(lily::SyntaxNode {
            range: type_node.range,
            value: LilySyntaxSymbol::TypeVariable {
                scope_declaration: scope_declaration,
                name: type_variable_value,
            },
        }),
    }
}

#[derive(Clone, Copy, Debug)]
enum LocalBindingOrigin {
    PatternVariable(lsp_types::Range),
    LocalDeclaredVariable { name_range: lsp_types::Range },
}

fn lily_syntax_expression_find_symbol_at_position<'a>(
    type_aliases: &std::collections::HashMap<lily::Name, lily::TypeAliasInfo>,
    choice_types: &std::collections::HashMap<lily::Name, lily::ChoiceTypeInfo>,
    variable_declarations: &std::collections::HashMap<
        lily::Name,
        lily::CompiledVariableDeclarationInfo,
    >,
    scope_declaration: &'a lily::SyntaxDeclaration,
    mut local_bindings: std::collections::HashMap<&'a str, LilyLocalBindingInfo<'a>>,
    expression_node: lily::SyntaxNode<&'a lily::SyntaxExpression>,
    position: lsp_types::Position,
) -> std::ops::ControlFlow<
    lily::SyntaxNode<LilySyntaxSymbol<'a>>,
    std::collections::HashMap<&'a str, LilyLocalBindingInfo<'a>>,
> {
    if !lsp_range_includes_position(expression_node.range, position) {
        return std::ops::ControlFlow::Continue(local_bindings);
    }
    match expression_node.value {
        lily::SyntaxExpression::Char(_) => std::ops::ControlFlow::Continue(local_bindings),
        lily::SyntaxExpression::Dec(_) => std::ops::ControlFlow::Continue(local_bindings),
        lily::SyntaxExpression::Unt(_) => std::ops::ControlFlow::Continue(local_bindings),
        lily::SyntaxExpression::Int(_) => std::ops::ControlFlow::Continue(local_bindings),
        lily::SyntaxExpression::String { .. } => std::ops::ControlFlow::Continue(local_bindings),
        lily::SyntaxExpression::VariableOrCall {
            variable: variable_node,
            arguments,
        } => {
            if lsp_range_includes_position(variable_node.range, position) {
                return std::ops::ControlFlow::Break(lily::SyntaxNode {
                    value: LilySyntaxSymbol::Variable {
                        name: &variable_node.value,
                        local_bindings: local_bindings,
                    },
                    range: variable_node.range,
                });
            }
            arguments
                .iter()
                .try_fold(local_bindings, |local_bindings, argument_node| {
                    lily_syntax_expression_find_symbol_at_position(
                        type_aliases,
                        choice_types,
                        variable_declarations,
                        scope_declaration,
                        local_bindings,
                        lily::syntax_node_as_ref(argument_node),
                        position,
                    )
                })
        }
        lily::SyntaxExpression::DotCall {
            argument0: argument0_node,
            dot_key_symbol_range,
            function_variable: maybe_variable_node,
            argument1_up,
        } => {
            match maybe_variable_node {
                None => {
                    if position == dot_key_symbol_range.end {
                        static lily_name_empty: lily::Name = lily::Name::from_static("");
                        return std::ops::ControlFlow::Break(lily::SyntaxNode {
                            value: LilySyntaxSymbol::Variable {
                                name: &lily_name_empty,
                                local_bindings: local_bindings,
                            },
                            range: lsp_types::Range {
                                start: position,
                                end: position,
                            },
                        });
                    }
                }
                Some(variable_node) => {
                    if lsp_range_includes_position(variable_node.range, position) {
                        return std::ops::ControlFlow::Break(lily::SyntaxNode {
                            value: LilySyntaxSymbol::Variable {
                                name: &variable_node.value,
                                local_bindings: local_bindings,
                            },
                            range: variable_node.range,
                        });
                    }
                }
            }
            std::iter::once(lily::syntax_node_unbox(argument0_node))
                .chain(argument1_up.iter().map(lily::syntax_node_as_ref))
                .try_fold(local_bindings, |local_bindings, argument_node| {
                    lily_syntax_expression_find_symbol_at_position(
                        type_aliases,
                        choice_types,
                        variable_declarations,
                        scope_declaration,
                        local_bindings,
                        argument_node,
                        position,
                    )
                })
        }
        lily::SyntaxExpression::Match {
            matched: matched_node,
            cases,
        } => {
            local_bindings = lily_syntax_expression_find_symbol_at_position(
                type_aliases,
                choice_types,
                variable_declarations,
                scope_declaration,
                local_bindings,
                lily::syntax_node_unbox(matched_node),
                position,
            )?;
            cases
                .iter()
                .try_fold(local_bindings, |mut local_bindings, case| {
                    if let Some(case_pattern_node) = &case.pattern
                        && let Some(found_symbol) = lily_syntax_pattern_find_symbol_at_position(
                            type_aliases,
                            choice_types,
                            scope_declaration,
                            case.result.as_ref().map(lily::syntax_node_as_ref),
                            lily::syntax_node_as_ref(case_pattern_node),
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
                            lily_syntax_pattern_bindings_into(
                                &mut local_bindings,
                                type_aliases,
                                choice_types,
                                lily::syntax_node_as_ref(case_result_node),
                                lily::syntax_node_as_ref(case_pattern_node),
                            );
                        }
                        lily_syntax_expression_find_symbol_at_position(
                            type_aliases,
                            choice_types,
                            variable_declarations,
                            scope_declaration,
                            local_bindings,
                            lily::syntax_node_as_ref(case_result_node),
                            position,
                        )
                    } else {
                        std::ops::ControlFlow::Continue(local_bindings)
                    }
                })
        }
        lily::SyntaxExpression::Lambda {
            parameters,
            arrow_key_symbol_range: _,
            result: maybe_result,
        } => {
            if let Some(found_symbol) = parameters.iter().find_map(|parameter| {
                lily_syntax_pattern_find_symbol_at_position(
                    type_aliases,
                    choice_types,
                    scope_declaration,
                    maybe_result.as_ref().map(lily::syntax_node_unbox),
                    lily::syntax_node_as_ref(parameter),
                    position,
                )
            }) {
                return std::ops::ControlFlow::Break(found_symbol);
            }
            match maybe_result {
                Some(result_node) => {
                    for parameter_node in parameters {
                        lily_syntax_pattern_bindings_into(
                            &mut local_bindings,
                            type_aliases,
                            choice_types,
                            lily::syntax_node_unbox(result_node),
                            lily::syntax_node_as_ref(parameter_node),
                        );
                    }
                    lily_syntax_expression_find_symbol_at_position(
                        type_aliases,
                        choice_types,
                        variable_declarations,
                        scope_declaration,
                        local_bindings,
                        lily::syntax_node_unbox(result_node),
                        position,
                    )
                }
                None => std::ops::ControlFlow::Continue(local_bindings),
            }
        }
        lily::SyntaxExpression::AfterLocalVariable {
            declaration: maybe_declaration,
            result: maybe_result,
        } => {
            if let Some(local_declaration_node) = maybe_declaration {
                local_bindings = lily_syntax_local_declaration_find_symbol_at_position(
                    type_aliases,
                    choice_types,
                    variable_declarations,
                    local_bindings,
                    scope_declaration,
                    maybe_result.as_ref().map(lily::syntax_node_unbox),
                    lily::syntax_node_as_ref(local_declaration_node),
                    position,
                )?;
            }
            match maybe_result {
                Some(result_node) => {
                    if let Some(local_declaration_node) = maybe_declaration {
                        local_bindings.insert(
                            &local_declaration_node.value.name.value,
                            lily_syntax_local_declaration_introduced_binding_info(
                                &local_bindings,
                                type_aliases,
                                choice_types,
                                variable_declarations,
                                lily::syntax_node_unbox(result_node),
                                &local_declaration_node.value,
                            ),
                        );
                    }
                    lily_syntax_expression_find_symbol_at_position(
                        type_aliases,
                        choice_types,
                        variable_declarations,
                        scope_declaration,
                        local_bindings,
                        lily::syntax_node_unbox(result_node),
                        position,
                    )
                }
                None => std::ops::ControlFlow::Continue(local_bindings),
            }
        }
        lily::SyntaxExpression::Vec(elements) => {
            elements
                .iter()
                .try_fold(local_bindings, |local_bindings, element| {
                    lily_syntax_expression_find_symbol_at_position(
                        type_aliases,
                        choice_types,
                        variable_declarations,
                        scope_declaration,
                        local_bindings,
                        lily::syntax_node_as_ref(element),
                        position,
                    )
                })
        }
        lily::SyntaxExpression::Parenthesized(None) => {
            std::ops::ControlFlow::Continue(local_bindings)
        }
        lily::SyntaxExpression::Parenthesized(Some(in_parens)) => {
            lily_syntax_expression_find_symbol_at_position(
                type_aliases,
                choice_types,
                variable_declarations,
                scope_declaration,
                local_bindings,
                lily::syntax_node_unbox(in_parens),
                position,
            )
        }
        lily::SyntaxExpression::WithComment {
            comment: _,
            expression: maybe_expression_after_comment,
        } => match maybe_expression_after_comment {
            None => std::ops::ControlFlow::Continue(local_bindings),
            Some(expression_node_after_comment) => lily_syntax_expression_find_symbol_at_position(
                type_aliases,
                choice_types,
                variable_declarations,
                scope_declaration,
                local_bindings,
                lily::syntax_node_unbox(expression_node_after_comment),
                position,
            ),
        },
        lily::SyntaxExpression::Typed {
            type_: maybe_type,
            closing_colon_range: _,
            expression: maybe_expression_in_typed,
        } => {
            if let Some(found) = maybe_type.as_ref().and_then(|type_node| {
                lily_syntax_type_find_symbol_at_position(
                    type_aliases,
                    choice_types,
                    scope_declaration,
                    lily::syntax_node_as_ref(type_node),
                    position,
                )
            }) {
                return std::ops::ControlFlow::Break(found);
            }
            match maybe_expression_in_typed {
                None => std::ops::ControlFlow::Continue(local_bindings),
                Some(expression_node_in_typed) => match expression_node_in_typed.value.as_ref() {
                    lily::SyntaxExpression::Variant {
                        name: name_node,
                        value: maybe_value,
                    } => {
                        if lsp_range_includes_position(name_node.range, position) {
                            return std::ops::ControlFlow::Break(lily::SyntaxNode {
                                value: LilySyntaxSymbol::Variant {
                                    name: &name_node.value,
                                    type_: maybe_type.as_ref().map(|n| &n.value),
                                },
                                range: name_node.range,
                            });
                        }
                        match maybe_value {
                            Some(value_node) => lily_syntax_expression_find_symbol_at_position(
                                type_aliases,
                                choice_types,
                                variable_declarations,
                                scope_declaration,
                                local_bindings,
                                lily::syntax_node_unbox(value_node),
                                position,
                            ),
                            None => std::ops::ControlFlow::Continue(local_bindings),
                        }
                    }
                    other_expression_in_typed => lily_syntax_expression_find_symbol_at_position(
                        type_aliases,
                        choice_types,
                        variable_declarations,
                        scope_declaration,
                        local_bindings,
                        lily::SyntaxNode {
                            range: expression_node_in_typed.range,
                            value: other_expression_in_typed,
                        },
                        position,
                    ),
                },
            }
        }
        lily::SyntaxExpression::Variant {
            name: name_node,
            value: maybe_value,
        } => {
            if lsp_range_includes_position(name_node.range, position) {
                return std::ops::ControlFlow::Break(lily::SyntaxNode {
                    value: LilySyntaxSymbol::Variant {
                        name: &name_node.value,
                        type_: None,
                    },
                    range: name_node.range,
                });
            }
            match maybe_value {
                Some(value_node) => lily_syntax_expression_find_symbol_at_position(
                    type_aliases,
                    choice_types,
                    variable_declarations,
                    scope_declaration,
                    local_bindings,
                    lily::syntax_node_unbox(value_node),
                    position,
                ),
                None => std::ops::ControlFlow::Continue(local_bindings),
            }
        }
        lily::SyntaxExpression::Record(fields) => std::ops::ControlFlow::Break(
            fields
                .iter()
                .try_fold(local_bindings, |local_bindings, field| {
                    lily_syntax_expression_field_find_symbol_at_position(
                        type_aliases,
                        choice_types,
                        variable_declarations,
                        scope_declaration,
                        local_bindings,
                        fields,
                        field,
                        position,
                    )
                })
                .break_value()
                .unwrap_or_else(|| lily::SyntaxNode {
                    range: lsp_types::Range {
                        start: position,
                        end: position,
                    },
                    value: LilySyntaxSymbol::InRecord {
                        fields_sorted: sorted_field_names(
                            fields.iter().map(|record_field| &record_field.name.value),
                        ),
                    },
                }),
        ),
        lily::SyntaxExpression::RecordUpdate {
            record: maybe_record,
            spread_key_symbol_range: _,
            fields,
        } => {
            if let Some(record_node) = maybe_record
                && lsp_range_includes_position(record_node.range, position)
            {
                return lily_syntax_expression_find_symbol_at_position(
                    type_aliases,
                    choice_types,
                    variable_declarations,
                    scope_declaration,
                    local_bindings,
                    lily::syntax_node_unbox(record_node),
                    position,
                );
            }
            fields
                .iter()
                .try_fold(local_bindings, |local_bindings, field| {
                    lily_syntax_expression_field_find_symbol_at_position(
                        type_aliases,
                        choice_types,
                        variable_declarations,
                        scope_declaration,
                        local_bindings,
                        fields,
                        field,
                        position,
                    )
                })
        }
    }
}
fn lily_syntax_expression_field_find_symbol_at_position<'a>(
    type_aliases: &std::collections::HashMap<lily::Name, lily::TypeAliasInfo>,
    choice_types: &std::collections::HashMap<lily::Name, lily::ChoiceTypeInfo>,
    variable_declarations: &std::collections::HashMap<
        lily::Name,
        lily::CompiledVariableDeclarationInfo,
    >,
    scope_declaration: &'a lily::SyntaxDeclaration,
    local_bindings: std::collections::HashMap<&'a str, LilyLocalBindingInfo<'a>>,
    fields: &[lily::SyntaxExpressionField],
    field: &'a lily::SyntaxExpressionField,
    position: lsp_types::Position,
) -> std::ops::ControlFlow<
    lily::SyntaxNode<LilySyntaxSymbol<'a>>,
    std::collections::HashMap<&'a str, LilyLocalBindingInfo<'a>>,
> {
    if lsp_range_includes_position(field.name.range, position) {
        return std::ops::ControlFlow::Break(lily::SyntaxNode {
            value: LilySyntaxSymbol::Field {
                name: &field.name.value,
                value_type: field.value.as_ref().and_then(|field_value_node| {
                    lily::syntax_expression_type_with(
                        type_aliases,
                        choice_types,
                        variable_declarations,
                        std::rc::Rc::new(
                            local_bindings
                                .iter()
                                .map(|(&binding_name, binding_info)| {
                                    (binding_name, binding_info.type_.clone())
                                })
                                .collect::<std::collections::HashMap<_, _>>(),
                        ),
                        lily::syntax_node_as_ref(field_value_node),
                    )
                }),
                fields_sorted: sorted_field_names(
                    fields.iter().map(|record_field| &record_field.name.value),
                ),
            },
            range: field.name.range,
        });
    }
    match &field.value {
        Some(field_value_node) => lily_syntax_expression_find_symbol_at_position(
            type_aliases,
            choice_types,
            variable_declarations,
            scope_declaration,
            local_bindings,
            lily::syntax_node_as_ref(field_value_node),
            position,
        ),
        None => std::ops::ControlFlow::Continue(local_bindings),
    }
}

fn lily_syntax_local_declaration_find_symbol_at_position<'a>(
    type_aliases: &std::collections::HashMap<lily::Name, lily::TypeAliasInfo>,
    choice_types: &std::collections::HashMap<lily::Name, lily::ChoiceTypeInfo>,
    variable_declarations: &std::collections::HashMap<
        lily::Name,
        lily::CompiledVariableDeclarationInfo,
    >,
    local_bindings: std::collections::HashMap<&'a str, LilyLocalBindingInfo<'a>>,
    scope_declaration: &'a lily::SyntaxDeclaration,
    scope_expression: Option<lily::SyntaxNode<&'a lily::SyntaxExpression>>,
    local_declaration_node: lily::SyntaxNode<&'a lily::SyntaxLocalVariableDeclaration>,
    position: lsp_types::Position,
) -> std::ops::ControlFlow<
    lily::SyntaxNode<LilySyntaxSymbol<'a>>,
    std::collections::HashMap<&'a str, LilyLocalBindingInfo<'a>>,
> {
    if !lsp_range_includes_position(local_declaration_node.range, position) {
        return std::ops::ControlFlow::Continue(local_bindings);
    }
    if lsp_range_includes_position(local_declaration_node.value.name.range, position) {
        return std::ops::ControlFlow::Break(lily::SyntaxNode {
            value: LilySyntaxSymbol::LocalVariableDeclarationName {
                name: &local_declaration_node.value.name.value,
                type_: local_declaration_node
                    .value
                    .result
                    .as_ref()
                    .and_then(|result_node| {
                        lily::syntax_expression_type_with(
                            type_aliases,
                            choice_types,
                            variable_declarations,
                            std::rc::Rc::new(
                                local_bindings
                                    .iter()
                                    .map(|(&binding_name, binding_info)| {
                                        (binding_name, binding_info.type_.clone())
                                    })
                                    .collect::<std::collections::HashMap<_, _>>(),
                            ),
                            lily::syntax_node_unbox(result_node),
                        )
                    }),
                scope_expression: scope_expression,
            },
            range: local_declaration_node.value.name.range,
        });
    }
    match &local_declaration_node.value.result {
        Some(result_node) => lily_syntax_expression_find_symbol_at_position(
            type_aliases,
            choice_types,
            variable_declarations,
            scope_declaration,
            local_bindings,
            lily::syntax_node_unbox(result_node),
            position,
        ),
        None => std::ops::ControlFlow::Continue(local_bindings),
    }
}

fn lily_syntax_pattern_bindings_into<'a>(
    bindings_so_far: &mut std::collections::HashMap<&'a str, LilyLocalBindingInfo<'a>>,
    type_aliases: &std::collections::HashMap<lily::Name, lily::TypeAliasInfo>,
    choice_types: &std::collections::HashMap<lily::Name, lily::ChoiceTypeInfo>,
    scope_expression: lily::SyntaxNode<&'a lily::SyntaxExpression>,
    pattern_node: lily::SyntaxNode<&'a lily::SyntaxPattern>,
) {
    match pattern_node.value {
        lily::SyntaxPattern::Char(_) => {}
        lily::SyntaxPattern::Unt(_) => {}
        lily::SyntaxPattern::Int(_) => {}
        lily::SyntaxPattern::String { .. } => {}
        lily::SyntaxPattern::Typed {
            type_: maybe_type,
            closing_colon_range: _,
            pattern: maybe_pattern_node_in_typed,
        } => {
            if let Some(pattern_node_in_typed) = maybe_pattern_node_in_typed {
                match pattern_node_in_typed.value.as_ref() {
                    lily::SyntaxPattern::Variable {
                        overwriting: _,
                        name: variable_name,
                    } => {
                        bindings_so_far.insert(
                            variable_name,
                            LilyLocalBindingInfo {
                                origin: LocalBindingOrigin::PatternVariable(
                                    pattern_node_in_typed.range,
                                ),
                                scope_expression: Some(scope_expression),
                                type_: maybe_type.as_ref().and_then(|type_node| {
                                    lily::syntax_type_to_type(
                                        &mut Vec::new(),
                                        type_aliases,
                                        choice_types,
                                        lily::syntax_node_as_ref(type_node),
                                    )
                                }),
                            },
                        );
                    }
                    other_in_typed => {
                        lily_syntax_pattern_bindings_into(
                            bindings_so_far,
                            type_aliases,
                            choice_types,
                            scope_expression,
                            lily::SyntaxNode {
                                range: pattern_node_in_typed.range,
                                value: other_in_typed,
                            },
                        );
                    }
                }
            }
        }
        lily::SyntaxPattern::Ignored => {}
        lily::SyntaxPattern::Variable {
            overwriting: _,
            name: variable_name,
        } => {
            bindings_so_far.insert(
                variable_name,
                LilyLocalBindingInfo {
                    origin: LocalBindingOrigin::PatternVariable(pattern_node.range),
                    scope_expression: Some(scope_expression),
                    type_: None,
                },
            );
        }
        lily::SyntaxPattern::Variant {
            name: _,
            value: maybe_value,
        } => {
            if let Some(value_node) = maybe_value {
                lily_syntax_pattern_bindings_into(
                    bindings_so_far,
                    type_aliases,
                    choice_types,
                    scope_expression,
                    lily::syntax_node_unbox(value_node),
                );
            }
        }
        lily::SyntaxPattern::WithComment {
            comment: _,
            pattern: maybe_pattern_after_comment,
        } => {
            if let Some(pattern_node_after_comment) = maybe_pattern_after_comment {
                lily_syntax_pattern_bindings_into(
                    bindings_so_far,
                    type_aliases,
                    choice_types,
                    scope_expression,
                    lily::syntax_node_unbox(pattern_node_after_comment),
                );
            }
        }
        lily::SyntaxPattern::Record(fields) => {
            for field in fields {
                if let Some(field_value_node) = &field.value {
                    lily_syntax_pattern_bindings_into(
                        bindings_so_far,
                        type_aliases,
                        choice_types,
                        scope_expression,
                        lily::syntax_node_as_ref(field_value_node),
                    );
                }
            }
        }
    }
}
fn lily_syntax_local_declaration_introduced_binding_info<'a>(
    bindings_so_far: &std::collections::HashMap<&'a str, LilyLocalBindingInfo<'a>>,
    type_aliases: &std::collections::HashMap<lily::Name, lily::TypeAliasInfo>,
    choice_types: &std::collections::HashMap<lily::Name, lily::ChoiceTypeInfo>,
    variable_declarations: &std::collections::HashMap<
        lily::Name,
        lily::CompiledVariableDeclarationInfo,
    >,
    scope_expression: lily::SyntaxNode<&'a lily::SyntaxExpression>,
    local_declaration: &'a lily::SyntaxLocalVariableDeclaration,
) -> LilyLocalBindingInfo<'a> {
    LilyLocalBindingInfo {
        scope_expression: Some(scope_expression),
        origin: LocalBindingOrigin::LocalDeclaredVariable {
            name_range: local_declaration.name.range,
        },
        type_: local_declaration.result.as_ref().and_then(|result_node| {
            lily::syntax_expression_type_with(
                type_aliases,
                choice_types,
                variable_declarations,
                // this is inefficient to do for every local variable declaration
                std::rc::Rc::new(
                    bindings_so_far
                        .iter()
                        .map(|(&binding_name, binding_info)| {
                            (binding_name, binding_info.type_.clone())
                        })
                        .collect::<std::collections::HashMap<_, _>>(),
                ),
                lily::syntax_node_unbox(result_node),
            )
        }),
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum LilySymbolToReference<'a> {
    TypeVariable(&'a lily::Name),
    // type is tracked separately from VariableOrVariant because e.g. variants and
    // type names are allowed to overlap
    Type {
        name: &'a lily::Name,
        including_declaration_name: bool,
    },
    Variable {
        name: &'a lily::Name,
        including_declaration_name: bool,
    },
    Variant {
        origin_type_name: Option<&'a lily::Name>,
        name: &'a lily::Name,
        including_declaration_name: bool,
    },
    LocalBinding {
        name: &'a lily::Name,
        including_local_declaration_name: bool,
    },
    Field {
        name: &'a lily::Name,
        fields_sorted: &'a [lily::Name],
    },
}

fn lily_syntax_project_uses_of_symbol_into(
    uses_so_far: &mut Vec<lsp_types::Range>,
    type_aliases: &std::collections::HashMap<lily::Name, lily::TypeAliasInfo>,
    syntax_project: &lily::SyntaxProject,
    symbol_to_collect_uses_of: LilySymbolToReference,
) {
    for documented_declaration in syntax_project
        .declarations
        .iter()
        .filter_map(|declaration_or_err| declaration_or_err.as_ref().ok())
    {
        if let Some(declaration_node) = &documented_declaration.declaration {
            lily_syntax_declaration_uses_of_symbol_into(
                uses_so_far,
                type_aliases,
                &declaration_node.value,
                symbol_to_collect_uses_of,
            );
        }
    }
}

fn lily_syntax_declaration_uses_of_symbol_into(
    uses_so_far: &mut Vec<lsp_types::Range>,
    type_aliases: &std::collections::HashMap<lily::Name, lily::TypeAliasInfo>,
    syntax_declaration: &lily::SyntaxDeclaration,
    symbol_to_collect_uses_of: LilySymbolToReference,
) {
    match syntax_declaration {
        lily::SyntaxDeclaration::ChoiceType {
            name: maybe_choice_type_name,
            parameters,
            variants,
        } => {
            if let Some(name_node) = maybe_choice_type_name
                && symbol_to_collect_uses_of
                    == (LilySymbolToReference::Type {
                        name: &name_node.value,
                        including_declaration_name: true,
                    })
            {
                uses_so_far.push(name_node.range);
            }
            'parameter_traversal: for parameter_node in parameters {
                if symbol_to_collect_uses_of
                    == LilySymbolToReference::TypeVariable(&parameter_node.value)
                {
                    uses_so_far.push(parameter_node.range);
                    break 'parameter_traversal;
                }
            }
            for variant in variants {
                if let LilySymbolToReference::Variant {
                    name: variant_to_collect_uses_of_name,
                    including_declaration_name: true,
                    origin_type_name: variant_to_collect_uses_of_maybe_origin_type,
                } = symbol_to_collect_uses_of
                    && let Some(variant_name_node) = &variant.name
                    && variant_to_collect_uses_of_name == &variant_name_node.value
                    && variant_to_collect_uses_of_maybe_origin_type.is_none_or(
                        |variant_to_collect_uses_of_origin_type| {
                            maybe_choice_type_name
                                .as_ref()
                                .is_none_or(|choice_type_name_node| {
                                    variant_to_collect_uses_of_origin_type
                                        == &choice_type_name_node.value
                                })
                        },
                    )
                {
                    uses_so_far.push(variant_name_node.range);
                    return;
                }
                for variant0_value in variant.value.iter() {
                    lily_syntax_type_uses_of_symbol_into(
                        uses_so_far,
                        lily::syntax_node_as_ref(variant0_value),
                        symbol_to_collect_uses_of,
                    );
                }
            }
        }
        lily::SyntaxDeclaration::TypeAlias {
            type_keyword_range: _,
            name: maybe_name,
            parameters,
            equals_key_symbol_range: _,
            type_: maybe_type,
        } => {
            if let Some(name_node) = maybe_name
                && (symbol_to_collect_uses_of
                    == (LilySymbolToReference::Type {
                        name: &name_node.value,

                        including_declaration_name: true,
                    }))
            {
                uses_so_far.push(name_node.range);
            }
            'parameter_traversal: for parameter_node in parameters {
                if symbol_to_collect_uses_of
                    == LilySymbolToReference::TypeVariable(&parameter_node.value)
                {
                    uses_so_far.push(parameter_node.range);
                    break 'parameter_traversal;
                }
            }
            if let Some(type_node) = maybe_type {
                lily_syntax_type_uses_of_symbol_into(
                    uses_so_far,
                    lily::syntax_node_as_ref(type_node),
                    symbol_to_collect_uses_of,
                );
            }
        }
        lily::SyntaxDeclaration::Variable {
            name: name_node,
            result: maybe_result,
        } => {
            if symbol_to_collect_uses_of
                == (LilySymbolToReference::Variable {
                    name: &name_node.value,
                    including_declaration_name: true,
                })
            {
                uses_so_far.push(name_node.range);
            }
            if let Some(result_node) = maybe_result {
                lily_syntax_expression_uses_of_symbol_into(
                    uses_so_far,
                    type_aliases,
                    &[],
                    lily::syntax_node_as_ref(result_node),
                    symbol_to_collect_uses_of,
                );
            }
        }
    }
}

fn lily_syntax_type_uses_of_symbol_into(
    uses_so_far: &mut Vec<lsp_types::Range>,
    type_node: lily::SyntaxNode<&lily::SyntaxType>,
    symbol_to_collect_uses_of: LilySymbolToReference,
) {
    match type_node.value {
        lily::SyntaxType::Construct {
            name: variable,
            arguments,
        } => {
            if let LilySymbolToReference::Type {
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
                lily_syntax_type_uses_of_symbol_into(
                    uses_so_far,
                    lily::syntax_node_as_ref(argument),
                    symbol_to_collect_uses_of,
                );
            }
        }
        lily::SyntaxType::Function {
            inputs,
            arrow_key_symbol_range: _,
            output: maybe_output,
        } => {
            for input in inputs {
                lily_syntax_type_uses_of_symbol_into(
                    uses_so_far,
                    lily::syntax_node_as_ref(input),
                    symbol_to_collect_uses_of,
                );
            }
            if let Some(output_node) = maybe_output {
                lily_syntax_type_uses_of_symbol_into(
                    uses_so_far,
                    lily::syntax_node_unbox(output_node),
                    symbol_to_collect_uses_of,
                );
            }
        }
        lily::SyntaxType::Parenthesized(None) => {}
        lily::SyntaxType::Parenthesized(Some(in_parens)) => {
            lily_syntax_type_uses_of_symbol_into(
                uses_so_far,
                lily::syntax_node_unbox(in_parens),
                symbol_to_collect_uses_of,
            );
        }
        lily::SyntaxType::WithComment {
            comment: _,
            type_: maybe_type_after_comment,
        } => {
            if let Some(type_node_after_comment) = maybe_type_after_comment {
                lily_syntax_type_uses_of_symbol_into(
                    uses_so_far,
                    lily::syntax_node_unbox(type_node_after_comment),
                    symbol_to_collect_uses_of,
                );
            }
        }
        lily::SyntaxType::Record(fields) => {
            if let LilySymbolToReference::Field {
                name: field_symbol_name,
                fields_sorted: symbol_fields_sorted,
            } = symbol_to_collect_uses_of
                && fields.len() == symbol_fields_sorted.len()
                && fields
                    .iter()
                    .all(|field| symbol_fields_sorted.contains(&field.name.value))
                && let Some(field_symbol_use) = fields
                    .iter()
                    .find(|field| &field.name.value == field_symbol_name)
            {
                uses_so_far.push(field_symbol_use.name.range);
            }
            for field in fields {
                if let Some(field_value_node) = &field.value {
                    lily_syntax_type_uses_of_symbol_into(
                        uses_so_far,
                        lily::syntax_node_as_ref(field_value_node),
                        symbol_to_collect_uses_of,
                    );
                }
            }
        }
        lily::SyntaxType::Variable(variable) => {
            if symbol_to_collect_uses_of == LilySymbolToReference::TypeVariable(variable) {
                uses_so_far.push(type_node.range);
            }
        }
    }
}

fn lily_syntax_expression_uses_of_symbol_into(
    uses_so_far: &mut Vec<lsp_types::Range>,
    type_aliases: &std::collections::HashMap<lily::Name, lily::TypeAliasInfo>,
    local_bindings: &[&str],
    expression_node: lily::SyntaxNode<&lily::SyntaxExpression>,
    symbol_to_collect_uses_of: LilySymbolToReference,
) {
    match expression_node.value {
        lily::SyntaxExpression::Unt(_) => {}
        lily::SyntaxExpression::Int(_) => {}
        lily::SyntaxExpression::Dec(_) => {}
        lily::SyntaxExpression::Char(_) => {}
        lily::SyntaxExpression::String { .. } => {}
        lily::SyntaxExpression::VariableOrCall {
            variable: variable_node,
            arguments,
        } => {
            let variable_name: &str = variable_node.value.as_str();
            let variable_is_symbol_use: bool = match symbol_to_collect_uses_of {
                LilySymbolToReference::LocalBinding {
                    name: symbol_name,
                    including_local_declaration_name: _,
                } => symbol_name == variable_name,
                LilySymbolToReference::Variable {
                    name: symbol_name,
                    including_declaration_name: _,
                } => symbol_name == variable_name,
                _ => false,
            } && // skip if shadowed
                !local_bindings.contains(&variable_name);
            if variable_is_symbol_use {
                uses_so_far.push(variable_node.range);
            }
            for argument_node in arguments {
                lily_syntax_expression_uses_of_symbol_into(
                    uses_so_far,
                    type_aliases,
                    local_bindings,
                    lily::syntax_node_as_ref(argument_node),
                    symbol_to_collect_uses_of,
                );
            }
        }
        lily::SyntaxExpression::DotCall {
            argument0: argument0_node,
            dot_key_symbol_range: _,
            function_variable: maybe_variable_node,
            argument1_up,
        } => {
            lily_syntax_expression_uses_of_symbol_into(
                uses_so_far,
                type_aliases,
                local_bindings,
                lily::syntax_node_unbox(argument0_node),
                symbol_to_collect_uses_of,
            );
            if let Some(variable_node) = maybe_variable_node {
                let variable_name: &str = variable_node.value.as_str();
                let variable_is_symbol_use: bool = match symbol_to_collect_uses_of {
                    LilySymbolToReference::LocalBinding {
                        name: symbol_name,
                        including_local_declaration_name: _,
                    } => symbol_name == variable_name,
                    LilySymbolToReference::Variable {
                        name: symbol_name,
                        including_declaration_name: _,
                    } => symbol_name == variable_name,
                    _ => false,
                } && // skip if shadowed
                    !local_bindings.contains(&variable_name);
                if variable_is_symbol_use {
                    uses_so_far.push(variable_node.range);
                }
            }
            for argument_node in argument1_up {
                lily_syntax_expression_uses_of_symbol_into(
                    uses_so_far,
                    type_aliases,
                    local_bindings,
                    lily::syntax_node_as_ref(argument_node),
                    symbol_to_collect_uses_of,
                );
            }
        }
        lily::SyntaxExpression::Match {
            matched: matched_node,
            cases,
        } => {
            lily_syntax_expression_uses_of_symbol_into(
                uses_so_far,
                type_aliases,
                local_bindings,
                lily::syntax_node_unbox(matched_node),
                symbol_to_collect_uses_of,
            );
            for case in cases {
                if let Some(case_pattern_node) = &case.pattern {
                    lily_syntax_pattern_uses_of_symbol_into(
                        uses_so_far,
                        type_aliases,
                        lily::syntax_node_as_ref(case_pattern_node),
                        symbol_to_collect_uses_of,
                    );
                }
                if let Some(case_result_node) = &case.result {
                    let mut local_bindings_including_from_case_pattern: Vec<&str> =
                        local_bindings.to_vec();
                    if let Some(case_pattern_node) = &case.pattern {
                        lily_syntax_pattern_binding_names_into(
                            &mut local_bindings_including_from_case_pattern,
                            lily::syntax_node_as_ref(case_pattern_node),
                        );
                    }
                    lily_syntax_expression_uses_of_symbol_into(
                        uses_so_far,
                        type_aliases,
                        &local_bindings_including_from_case_pattern,
                        lily::syntax_node_as_ref(case_result_node),
                        symbol_to_collect_uses_of,
                    );
                }
            }
        }
        lily::SyntaxExpression::Lambda {
            parameters,
            arrow_key_symbol_range: _,
            result: maybe_result,
        } => {
            for parameter_node in parameters {
                lily_syntax_pattern_uses_of_symbol_into(
                    uses_so_far,
                    type_aliases,
                    lily::syntax_node_as_ref(parameter_node),
                    symbol_to_collect_uses_of,
                );
            }
            if let Some(result_node) = maybe_result {
                let mut local_bindings_including_from_lambda_parameters: Vec<&str> =
                    local_bindings.to_vec();
                for parameter_node in parameters {
                    lily_syntax_pattern_binding_names_into(
                        &mut local_bindings_including_from_lambda_parameters,
                        lily::syntax_node_as_ref(parameter_node),
                    );
                }
                lily_syntax_expression_uses_of_symbol_into(
                    uses_so_far,
                    type_aliases,
                    &local_bindings_including_from_lambda_parameters,
                    lily::syntax_node_unbox(result_node),
                    symbol_to_collect_uses_of,
                );
            }
        }
        lily::SyntaxExpression::AfterLocalVariable {
            declaration: maybe_declaration,
            result: maybe_result,
        } => {
            let mut local_bindings_including_local_declaration_introduced: Vec<&str> =
                local_bindings.to_vec();
            if let Some(local_declaration_node) = maybe_declaration {
                if let Some(result_node) = &local_declaration_node.value.result {
                    lily_syntax_expression_uses_of_symbol_into(
                        uses_so_far,
                        type_aliases,
                        local_bindings,
                        lily::syntax_node_unbox(result_node),
                        symbol_to_collect_uses_of,
                    );
                }
                local_bindings_including_local_declaration_introduced
                    .push(&local_declaration_node.value.name.value);
            }
            if let Some(result) = maybe_result {
                lily_syntax_expression_uses_of_symbol_into(
                    uses_so_far,
                    type_aliases,
                    &local_bindings_including_local_declaration_introduced,
                    lily::syntax_node_unbox(result),
                    symbol_to_collect_uses_of,
                );
            }
        }
        lily::SyntaxExpression::Vec(elements) => {
            for element_node in elements {
                lily_syntax_expression_uses_of_symbol_into(
                    uses_so_far,
                    type_aliases,
                    local_bindings,
                    lily::syntax_node_as_ref(element_node),
                    symbol_to_collect_uses_of,
                );
            }
        }
        lily::SyntaxExpression::Parenthesized(None) => {}
        lily::SyntaxExpression::Parenthesized(Some(in_parens)) => {
            lily_syntax_expression_uses_of_symbol_into(
                uses_so_far,
                type_aliases,
                local_bindings,
                lily::syntax_node_unbox(in_parens),
                symbol_to_collect_uses_of,
            );
        }
        lily::SyntaxExpression::WithComment {
            comment: _,
            expression: maybe_expression_after_comment,
        } => {
            if let Some(expression_node_after_comment) = maybe_expression_after_comment {
                lily_syntax_expression_uses_of_symbol_into(
                    uses_so_far,
                    type_aliases,
                    local_bindings,
                    lily::syntax_node_unbox(expression_node_after_comment),
                    symbol_to_collect_uses_of,
                );
            }
        }
        lily::SyntaxExpression::Typed {
            type_: maybe_type,
            closing_colon_range: _,
            expression: maybe_expression_in_typed,
        } => {
            if let Some(type_node) = maybe_type {
                lily_syntax_type_uses_of_symbol_into(
                    uses_so_far,
                    lily::syntax_node_as_ref(type_node),
                    symbol_to_collect_uses_of,
                );
            }
            if let Some(expression_node_in_typed) = maybe_expression_in_typed {
                match expression_node_in_typed.value.as_ref() {
                    lily::SyntaxExpression::Variant {
                        name: name_node,
                        value: maybe_value,
                    } => {
                        if let LilySymbolToReference::Variant {
                            name: symbol_name,
                            including_declaration_name: _,
                            origin_type_name: variant_to_collect_uses_of_maybe_origin_type_name,
                        } = symbol_to_collect_uses_of
                            && symbol_name == name_node.value.as_str()
                            && let maybe_origin_choice_type_name =
                                maybe_type.as_ref().and_then(|type_node| {
                                    lily_syntax_type_to_choice_type(
                                        type_aliases,
                                        lily::syntax_node_as_ref(type_node),
                                    )
                                    .map(|(origin_choice_type_name, _)| origin_choice_type_name)
                                })
                            && variant_to_collect_uses_of_maybe_origin_type_name.is_none_or(
                                |variant_to_collect_uses_of_origin_type_name| {
                                    maybe_origin_choice_type_name.is_none_or(
                                        |origin_choice_type_name| {
                                            &origin_choice_type_name
                                                == variant_to_collect_uses_of_origin_type_name
                                        },
                                    )
                                },
                            )
                        {
                            uses_so_far.push(name_node.range);
                        }
                        if let Some(value_node) = maybe_value {
                            lily_syntax_expression_uses_of_symbol_into(
                                uses_so_far,
                                type_aliases,
                                local_bindings,
                                lily::syntax_node_unbox(value_node),
                                symbol_to_collect_uses_of,
                            );
                        }
                    }
                    other_expression_in_typed => {
                        lily_syntax_expression_uses_of_symbol_into(
                            uses_so_far,
                            type_aliases,
                            local_bindings,
                            lily::SyntaxNode {
                                range: expression_node_in_typed.range,
                                value: other_expression_in_typed,
                            },
                            symbol_to_collect_uses_of,
                        );
                    }
                }
            }
        }
        lily::SyntaxExpression::Variant {
            name: name_node,
            value: maybe_value,
        } => {
            if let LilySymbolToReference::Variant {
                name: symbol_name,
                including_declaration_name: _,
                origin_type_name: _,
            } = symbol_to_collect_uses_of
                && symbol_name == name_node.value.as_str()
            {
                uses_so_far.push(name_node.range);
            }
            if let Some(value_node) = maybe_value {
                lily_syntax_expression_uses_of_symbol_into(
                    uses_so_far,
                    type_aliases,
                    local_bindings,
                    lily::syntax_node_unbox(value_node),
                    symbol_to_collect_uses_of,
                );
            }
        }
        lily::SyntaxExpression::Record(fields) => {
            if let LilySymbolToReference::Field {
                name: field_symbol_name,
                fields_sorted: symbol_fields_sorted,
            } = symbol_to_collect_uses_of
                && fields.len() == symbol_fields_sorted.len()
                && fields
                    .iter()
                    .all(|field| symbol_fields_sorted.contains(&field.name.value))
                && let Some(field_symbol_use) = fields
                    .iter()
                    .find(|field| &field.name.value == field_symbol_name)
            {
                uses_so_far.push(field_symbol_use.name.range);
            }
            for field in fields {
                if let Some(field_value_node) = &field.value {
                    lily_syntax_expression_uses_of_symbol_into(
                        uses_so_far,
                        type_aliases,
                        local_bindings,
                        lily::syntax_node_as_ref(field_value_node),
                        symbol_to_collect_uses_of,
                    );
                }
            }
        }
        lily::SyntaxExpression::RecordUpdate {
            record: maybe_record,
            spread_key_symbol_range: _,
            fields,
        } => {
            if let Some(record_node) = maybe_record {
                lily_syntax_expression_uses_of_symbol_into(
                    uses_so_far,
                    type_aliases,
                    local_bindings,
                    lily::syntax_node_unbox(record_node),
                    symbol_to_collect_uses_of,
                );
            }
            for field in fields {
                if let Some(field_value_node) = &field.value {
                    lily_syntax_expression_uses_of_symbol_into(
                        uses_so_far,
                        type_aliases,
                        local_bindings,
                        lily::syntax_node_as_ref(field_value_node),
                        symbol_to_collect_uses_of,
                    );
                }
            }
        }
    }
}
fn lily_syntax_pattern_binding_names_into<'a>(
    bindings_so_far: &mut Vec<&'a str>,
    pattern_node: lily::SyntaxNode<&'a lily::SyntaxPattern>,
) {
    match pattern_node.value {
        lily::SyntaxPattern::Char(_) => {}
        lily::SyntaxPattern::Unt(_) => {}
        lily::SyntaxPattern::Int(_) => {}
        lily::SyntaxPattern::String { .. } => {}
        lily::SyntaxPattern::Typed {
            type_: _,
            closing_colon_range: _,
            pattern: maybe_pattern_node_in_typed,
        } => {
            if let Some(pattern_node_in_typed) = maybe_pattern_node_in_typed {
                lily_syntax_pattern_binding_names_into(
                    bindings_so_far,
                    lily::SyntaxNode {
                        range: pattern_node_in_typed.range,
                        value: &pattern_node_in_typed.value,
                    },
                );
            }
        }
        lily::SyntaxPattern::Ignored => {}
        lily::SyntaxPattern::Variable {
            overwriting: _,
            name: variable_name,
        } => {
            bindings_so_far.push(variable_name);
        }
        lily::SyntaxPattern::Variant {
            name: _,
            value: maybe_value,
        } => {
            if let Some(value_node) = maybe_value {
                lily_syntax_pattern_binding_names_into(
                    bindings_so_far,
                    lily::syntax_node_unbox(value_node),
                );
            }
        }
        lily::SyntaxPattern::WithComment {
            comment: _,
            pattern: maybe_pattern_after_comment,
        } => {
            if let Some(pattern_node_after_comment) = maybe_pattern_after_comment {
                lily_syntax_pattern_binding_names_into(
                    bindings_so_far,
                    lily::syntax_node_unbox(pattern_node_after_comment),
                );
            }
        }
        lily::SyntaxPattern::Record(fields) => {
            for field in fields {
                if let Some(field_value_node) = &field.value {
                    lily_syntax_pattern_binding_names_into(
                        bindings_so_far,
                        lily::syntax_node_as_ref(field_value_node),
                    );
                }
            }
        }
    }
}

fn lily_syntax_pattern_uses_of_symbol_into(
    uses_so_far: &mut Vec<lsp_types::Range>,
    type_aliases: &std::collections::HashMap<lily::Name, lily::TypeAliasInfo>,
    pattern_node: lily::SyntaxNode<&lily::SyntaxPattern>,
    symbol_to_collect_uses_of: LilySymbolToReference,
) {
    match pattern_node.value {
        lily::SyntaxPattern::Unt(_) => {}
        lily::SyntaxPattern::Int(_) => {}
        lily::SyntaxPattern::Char(_) => {}
        lily::SyntaxPattern::String { .. } => {}
        lily::SyntaxPattern::Typed {
            type_: maybe_type,
            closing_colon_range: _,
            pattern: maybe_pattern_node_in_typed,
        } => {
            if let Some(type_node) = maybe_type {
                lily_syntax_type_uses_of_symbol_into(
                    uses_so_far,
                    lily::syntax_node_as_ref(type_node),
                    symbol_to_collect_uses_of,
                );
            }
            if let Some(pattern_node_in_typed) = maybe_pattern_node_in_typed {
                match pattern_node_in_typed.value.as_ref() {
                    lily::SyntaxPattern::Variant {
                        name: variant_name_node,
                        value: maybe_value,
                    } => {
                        if let LilySymbolToReference::Variant {
                            name: variant_to_collect_uses_of_name,
                            including_declaration_name: _,
                            origin_type_name: variant_to_collect_uses_of_maybe_origin_type_name,
                        } = symbol_to_collect_uses_of
                            && variant_to_collect_uses_of_name == &variant_name_node.value
                            && let maybe_origin_choice_type_name =
                                maybe_type.as_ref().and_then(|type_node| {
                                    lily_syntax_type_to_choice_type(
                                        type_aliases,
                                        lily::syntax_node_as_ref(type_node),
                                    )
                                    .map(|(origin_choice_type_name, _)| origin_choice_type_name)
                                })
                            && variant_to_collect_uses_of_maybe_origin_type_name.is_none_or(
                                |variant_to_collect_uses_of_origin_type_name| {
                                    maybe_origin_choice_type_name.is_none_or(
                                        |origin_choice_type_name| {
                                            &origin_choice_type_name
                                                == variant_to_collect_uses_of_origin_type_name
                                        },
                                    )
                                },
                            )
                        {
                            uses_so_far.push(variant_name_node.range);
                        }
                        if let Some(value) = maybe_value {
                            lily_syntax_pattern_uses_of_symbol_into(
                                uses_so_far,
                                type_aliases,
                                lily::syntax_node_unbox(value),
                                symbol_to_collect_uses_of,
                            );
                        }
                    }
                    other_in_typed => {
                        lily_syntax_pattern_uses_of_symbol_into(
                            uses_so_far,
                            type_aliases,
                            lily::SyntaxNode {
                                range: pattern_node_in_typed.range,
                                value: other_in_typed,
                            },
                            symbol_to_collect_uses_of,
                        );
                    }
                }
            }
        }
        lily::SyntaxPattern::Variant {
            name: variant_name_node,
            value: maybe_value,
        } => {
            if let LilySymbolToReference::Variant {
                name: variant_to_collect_uses_of_name,
                including_declaration_name: _,
                origin_type_name: _,
            } = symbol_to_collect_uses_of
                && variant_to_collect_uses_of_name == &variant_name_node.value
            {
                uses_so_far.push(variant_name_node.range);
            }
            if let Some(value) = maybe_value {
                lily_syntax_pattern_uses_of_symbol_into(
                    uses_so_far,
                    type_aliases,
                    lily::syntax_node_unbox(value),
                    symbol_to_collect_uses_of,
                );
            }
        }
        lily::SyntaxPattern::Ignored => {}
        lily::SyntaxPattern::Variable { .. } => {}
        lily::SyntaxPattern::WithComment {
            comment: _,
            pattern: maybe_pattern_after_comment,
        } => {
            if let Some(pattern_node_after_comment) = maybe_pattern_after_comment {
                lily_syntax_pattern_uses_of_symbol_into(
                    uses_so_far,
                    type_aliases,
                    lily::syntax_node_unbox(pattern_node_after_comment),
                    symbol_to_collect_uses_of,
                );
            }
        }
        lily::SyntaxPattern::Record(fields) => {
            if let LilySymbolToReference::Field {
                name: field_symbol_name,
                fields_sorted: symbol_fields_sorted,
            } = symbol_to_collect_uses_of
                && fields.len() == symbol_fields_sorted.len()
                && fields
                    .iter()
                    .all(|field| symbol_fields_sorted.contains(&field.name.value))
                && let Some(field_symbol_use) = fields
                    .iter()
                    .find(|field| &field.name.value == field_symbol_name)
            {
                uses_so_far.push(field_symbol_use.name.range);
            }
            for value in fields.iter().filter_map(|field| field.value.as_ref()) {
                lily_syntax_pattern_uses_of_symbol_into(
                    uses_so_far,
                    type_aliases,
                    lily::syntax_node_as_ref(value),
                    symbol_to_collect_uses_of,
                );
            }
        }
    }
}

// // helpers

fn sorted_field_names<'a>(field_names: impl Iterator<Item = &'a lily::Name>) -> Vec<lily::Name> {
    let mut field_names_vec: Vec<lily::Name> = field_names.map(lily::Name::clone).collect();
    field_names_vec.sort_unstable();
    field_names_vec
}

fn lily_syntax_type_to_choice_type(
    type_aliases: &std::collections::HashMap<lily::Name, lily::TypeAliasInfo>,
    lily_type_node: lily::SyntaxNode<&lily::SyntaxType>,
) -> Option<(lily::Name, Vec<lily::SyntaxNode<lily::SyntaxType>>)> {
    match lily_type_node.value {
        lily::SyntaxType::WithComment {
            comment: _,
            type_: Some(after_comment_node),
        } => lily_syntax_type_to_choice_type(
            type_aliases,
            lily::syntax_node_unbox(after_comment_node),
        ),
        lily::SyntaxType::Parenthesized(Some(in_parens_node)) => {
            lily_syntax_type_to_choice_type(type_aliases, lily::syntax_node_unbox(in_parens_node))
        }
        lily::SyntaxType::Construct {
            name: name_node,
            arguments,
        } => match lily_syntax_type_resolve_while_type_alias(type_aliases, lily_type_node) {
            None => Some((name_node.value.clone(), arguments.clone())),
            Some(resolved) => {
                lily_syntax_type_to_choice_type(type_aliases, lily::syntax_node_as_ref(&resolved))
            }
        },
        _ => None,
    }
}
/// Keep peeling until the type is not a type alias anymore.
/// _Inner_ type aliases in a sub-part will not be resolved.
/// This will also not check for aliases inside parenthesized types or after comments
fn lily_syntax_type_resolve_while_type_alias(
    type_aliases: &std::collections::HashMap<lily::Name, lily::TypeAliasInfo>,
    type_node: lily::SyntaxNode<&lily::SyntaxType>,
) -> Option<lily::SyntaxNode<lily::SyntaxType>> {
    match type_node.value {
        lily::SyntaxType::Construct {
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
                        lily::SyntaxNode<&lily::SyntaxType>,
                    > = type_alias
                        .parameters
                        .iter()
                        .map(|n| n.value.as_str())
                        .zip(arguments.iter().map(lily::syntax_node_as_ref))
                        .collect::<std::collections::HashMap<_, _>>();
                    let peeled: lily::SyntaxNode<lily::SyntaxType> =
                        lily_syntax_type_replace_variables(
                            &type_parameter_replacements,
                            lily::syntax_node_as_ref(type_alias_type_node),
                        );
                    Some(
                        match lily_syntax_type_resolve_while_type_alias(
                            type_aliases,
                            lily::syntax_node_as_ref(&peeled),
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
fn lily_syntax_type_replace_variables(
    type_parameter_replacements: &std::collections::HashMap<
        &str,
        lily::SyntaxNode<&lily::SyntaxType>,
    >,
    type_node: lily::SyntaxNode<&lily::SyntaxType>,
) -> lily::SyntaxNode<lily::SyntaxType> {
    match type_node.value {
        lily::SyntaxType::Variable(variable) => {
            match type_parameter_replacements.get(variable.as_str()) {
                None => lily::syntax_node_map(type_node, lily::SyntaxType::clone),
                Some(&replacement_type_node) => {
                    lily::syntax_node_map(replacement_type_node, lily::SyntaxType::clone)
                }
            }
        }
        lily::SyntaxType::Parenthesized(maybe_in_parens) => match maybe_in_parens {
            None => lily::syntax_node_map(type_node, lily::SyntaxType::clone),
            Some(in_parens_node) => lily::SyntaxNode {
                range: type_node.range,
                value: lily::SyntaxType::Parenthesized(Some(lily::syntax_node_box(
                    lily_syntax_type_replace_variables(
                        type_parameter_replacements,
                        lily::syntax_node_unbox(in_parens_node),
                    ),
                ))),
            },
        },
        lily::SyntaxType::WithComment {
            comment: maybe_comment,
            type_: maybe_type,
        } => lily::SyntaxNode {
            range: type_node.range,
            value: lily::SyntaxType::WithComment {
                comment: maybe_comment.clone(),
                type_: maybe_type.as_ref().map(|after_comment_node| {
                    lily::syntax_node_box(lily_syntax_type_replace_variables(
                        type_parameter_replacements,
                        lily::syntax_node_unbox(after_comment_node),
                    ))
                }),
            },
        },
        lily::SyntaxType::Construct {
            name: name_node,
            arguments,
        } => lily::SyntaxNode {
            range: type_node.range,
            value: lily::SyntaxType::Construct {
                name: name_node.clone(),
                arguments: arguments
                    .iter()
                    .map(|argument_node| {
                        lily_syntax_type_replace_variables(
                            type_parameter_replacements,
                            lily::syntax_node_as_ref(argument_node),
                        )
                    })
                    .collect(),
            },
        },
        lily::SyntaxType::Record(fields) => lily::SyntaxNode {
            range: type_node.range,
            value: lily::SyntaxType::Record(
                fields
                    .iter()
                    .map(|field| lily::SyntaxTypeField {
                        name: field.name.clone(),
                        value: field.value.as_ref().map(|field_value_node| {
                            lily_syntax_type_replace_variables(
                                type_parameter_replacements,
                                lily::syntax_node_as_ref(field_value_node),
                            )
                        }),
                    })
                    .collect(),
            ),
        },
        lily::SyntaxType::Function {
            inputs,
            arrow_key_symbol_range: maybe_arrow_key_symbol_range,
            output: maybe_output,
        } => lily::SyntaxNode {
            range: type_node.range,
            value: lily::SyntaxType::Function {
                inputs: inputs
                    .iter()
                    .map(|argument_node| {
                        lily_syntax_type_replace_variables(
                            type_parameter_replacements,
                            lily::syntax_node_as_ref(argument_node),
                        )
                    })
                    .collect(),
                arrow_key_symbol_range: *maybe_arrow_key_symbol_range,
                output: maybe_output.as_ref().map(|after_comment_node| {
                    lily::syntax_node_box(lily_syntax_type_replace_variables(
                        type_parameter_replacements,
                        lily::syntax_node_unbox(after_comment_node),
                    ))
                }),
            },
        },
    }
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

fn str_lsp_range_to_range(str: &str, range: lsp_types::Range) -> std::ops::Range<usize> {
    let start_line_offset: usize =
        str_offset_after_n_lsp_linebreaks(str, range.start.line as usize);
    let start_offset: usize = start_line_offset
        + str_starting_utf8_length_for_utf16_length(
            &str[start_line_offset..],
            range.start.character as usize,
        );
    // can be optimized by only counting after the start line
    let end_line_offset: usize = str_offset_after_n_lsp_linebreaks(str, range.end.line as usize);
    let end_offset: usize = end_line_offset
        + str_starting_utf8_length_for_utf16_length(
            &str[end_line_offset..],
            range.end.character as usize,
        );
    start_offset..end_offset
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
fn string_replace_lsp_range(string: &mut String, range: lsp_types::Range, replacement: &str) {
    string.replace_range(str_lsp_range_to_range(string, range), replacement);
}
/// slightly faster version of `string_replace_lsp_range` for when you know the length
fn string_replace_lsp_range_for_length(
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

/// "polyfill" for the removed lsp_types::Uri::to_file_path (removed after 0.95.1)
/// Inspired by (thank you!): https://github.com/tower-lsp-community/tower-lsp-server/blob/ff1562a33bda1da55ef4edbfc9ee24ecd50f6807/src/uri_ext.rs
fn lsp_uri_to_file_path(uri: &lsp_types::Uri) -> Option<std::borrow::Cow<'_, std::path::Path>> {
    let Ok(path_as_str) = uri.path().as_estr().decode().into_string() else {
        return None;
    };
    let path_as_file_path: std::borrow::Cow<std::path::Path> = match path_as_str {
        std::borrow::Cow::Borrowed(str) => std::borrow::Cow::Borrowed(std::path::Path::new(str)),
        std::borrow::Cow::Owned(owned) => std::borrow::Cow::Owned(std::path::PathBuf::from(owned)),
    };
    if cfg!(windows) {
        let Some(authority) = uri.authority() else {
            return None;
        };
        let host = authority.host();
        if host.as_str().is_empty() {
            // assume file:/// → path includes leading /
            let path_with_leading_slash_str: std::borrow::Cow<str> =
                path_as_file_path.to_string_lossy();
            let Some(path_without_leading_slash) = path_with_leading_slash_str.get(1..) else {
                return None;
            };
            Some(std::borrow::Cow::Owned(std::path::PathBuf::from(
                path_without_leading_slash,
            )))
        } else {
            let mut full_file_path: std::path::PathBuf =
                std::path::PathBuf::from(format!("{host}:"));
            full_file_path.push(path_as_file_path);
            Some(std::borrow::Cow::Owned(full_file_path))
        }
    } else {
        Some(path_as_file_path)
    }
}
