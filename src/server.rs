use std::sync::Arc;

use log::{debug, error};
use rmcp::{
    ErrorData as McpError, RoleServer, ServerHandler, handler::server::wrapper::Parameters,
    model::*, service::RequestContext, tool, tool_handler, tool_router,
};

use firm_mcp::resources;
use firm_mcp::tools::{
    self, AddEntityParams, BuildParams, DslReferenceParams, FindSourceParams, GetParams,
    ListParams, QueryParams, ReadSourceParams, RelatedParams, ReplaceSourceParams,
    WriteSourceParams,
};
use firm_mcp::FirmMcpServer;

use crate::git::{self, GitConfig, GitError};

/// Remote MCP server that wraps FirmMcpServer with git commit/push on writes.
#[derive(Clone)]
pub struct RemoteFirmServer {
    firm: FirmMcpServer,
    git_config: GitConfig,
    git_lock: Arc<tokio::sync::Mutex<()>>,
    tool_router: rmcp::handler::server::router::tool::ToolRouter<RemoteFirmServer>,
}

#[tool_router]
impl RemoteFirmServer {
    pub fn new(firm: FirmMcpServer, git_config: GitConfig) -> Self {
        Self {
            firm,
            git_config,
            git_lock: Arc::new(tokio::sync::Mutex::new(())),
            tool_router: Self::tool_router(),
        }
    }

    /// Commit and push changes, logging errors. Returns a warning string on failure.
    async fn git_commit_and_push(&self, message: &str) -> Option<String> {
        let _guard = self.git_lock.lock().await;
        match git::commit_and_push(self.git_config.clone(), message).await {
            Ok(()) => None,
            Err(GitError::NothingToCommit) => None,
            Err(e) => {
                error!("Git commit/push failed: {}", e);
                Some(format!("Warning: git commit/push failed: {}", e))
            }
        }
    }

    // -- Read tools --

    #[tool(
        description = "List all entity IDs of a given type, or all schema names if type is 'schema'. \
        Returns only IDs/names for discovery purposes. Use 'get' to retrieve full details for a specific entity or schema, \
        or use 'query' to fetch details for multiple entities matching search criteria."
    )]
    async fn list(
        &self,
        Parameters(params): Parameters<ListParams>,
    ) -> Result<CallToolResult, McpError> {
        debug!("Tool: list, type={}", params.r#type);
        let state = self.firm.state().lock().await;
        Ok(tools::list::execute(&state.build, &params))
    }

    #[tool(description = "Get full details of a single entity or schema. \
        For entities: provide the entity type (e.g., 'person') and ID (e.g., 'john_doe'). \
        For schemas: use type='schema' and id=<schema_name> (e.g., id='person'). \
        Returns all fields and their values. Use 'list' first to discover available IDs.")]
    async fn get(
        &self,
        Parameters(params): Parameters<GetParams>,
    ) -> Result<CallToolResult, McpError> {
        debug!("Tool: get, type={}, id={}", params.r#type, params.id);
        let state = self.firm.state().lock().await;
        Ok(tools::get::execute(&state.build, &params))
    }

    #[tool(
        description = "Query entities using the Firm query language. Returns full details for all matching entities, \
        or an aggregated result when an aggregation clause is used. \
        Examples: 'from person', 'from task | where is_completed == false', \
        'from task | where is_completed == false and priority > 5', \
        'from invoice | where status == \"draft\" or status == \"sent\"', \
        'from person | where name contains \"John\" | limit 5', \
        'from task | count', 'from invoice | where status == \"sent\" | sum amount', \
        'from task | where is_completed == false | select @id, name, due_date'. \
        Use 'list' for a simple ID overview, or 'get' for a single entity's details."
    )]
    async fn query(
        &self,
        Parameters(params): Parameters<QueryParams>,
    ) -> Result<CallToolResult, McpError> {
        debug!("Tool: query, query={}", params.query);
        let state = self.firm.state().lock().await;
        Ok(tools::query::execute(&state.graph, &params))
    }

    #[tool(description = "Get IDs of entities related to a specific entity. \
        Returns entity IDs that reference or are referenced by the given entity. \
        Use 'direction' to filter: 'incoming' (entities that reference this one), \
        'outgoing' (entities this one references), or omit for both.")]
    async fn related(
        &self,
        Parameters(params): Parameters<RelatedParams>,
    ) -> Result<CallToolResult, McpError> {
        debug!(
            "Tool: related, type={}, id={}, direction={:?}",
            params.r#type, params.id, params.direction
        );
        let state = self.firm.state().lock().await;
        Ok(tools::related::execute(&state.graph, &params))
    }

    #[tool(description = "Find the source file path for an entity or schema. \
        Returns the relative path to the .firm file containing the definition. \
        Use this to locate where an entity or schema is defined before reading or editing the source file.")]
    async fn find_source(
        &self,
        Parameters(params): Parameters<FindSourceParams>,
    ) -> Result<CallToolResult, McpError> {
        debug!(
            "Tool: find_source, type={}, id={}",
            params.r#type, params.id
        );
        let state = self.firm.state().lock().await;
        Ok(tools::find_source::execute(
            &state.workspace,
            self.firm.workspace_path(),
            &params,
        ))
    }

    #[tool(description = "Read the raw DSL content of a .firm source file. \
        Provide the relative path to the file (e.g., 'schemas/person.firm', 'core/main.firm'). \
        Use 'find_source' first to locate the file path for a specific entity or schema.")]
    async fn read_source(
        &self,
        Parameters(params): Parameters<ReadSourceParams>,
    ) -> Result<CallToolResult, McpError> {
        debug!("Tool: read_source, path={}", params.path);
        Ok(tools::read_source::execute(
            self.firm.workspace_path(),
            &params,
        ))
    }

    #[tool(
        description = "Get reference documentation for the Firm DSL syntax and query language. \
        Use 'topic' parameter: 'dsl' for DSL syntax (entities, schemas, field types), \
        'query' for query language (from, where, related, order, limit, aggregations), \
        or 'all' for both (default). \
        Call this before writing or modifying .firm files to understand the correct syntax."
    )]
    async fn dsl_reference(
        &self,
        Parameters(params): Parameters<DslReferenceParams>,
    ) -> Result<CallToolResult, McpError> {
        debug!("Tool: dsl_reference, topic={}", params.topic);
        Ok(tools::dsl_reference::execute(&params))
    }

    // -- Build tool --

    #[tool(description = "Sync with remote and rebuild the workspace. \
        Fetches the latest state from the remote mcp branch (or main if mcp was deleted after a PR merge). \
        Returns the current status: number of entities and schemas if valid, \
        or validation errors if the workspace is broken. \
        Call this before starting work to ensure you have the latest data.")]
    async fn build(
        &self,
        #[allow(unused_variables)] Parameters(params): Parameters<BuildParams>,
    ) -> Result<CallToolResult, McpError> {
        debug!("Tool: build (with git sync)");

        // Fetch origin and reset local branch to match remote
        let mut warnings = Vec::new();
        if let Err(e) = git::clone_or_fetch(self.git_config.clone()).await {
            warnings.push(format!("Git fetch failed: {}", e));
        } else if let Err(e) = git::sync_branch(self.git_config.clone()).await {
            warnings.push(format!("Git sync failed: {}", e));
        }

        match self.firm.rebuild().await {
            Ok(_) => {
                let state = self.firm.state().lock().await;
                let mut result = tools::build::success_result(
                    state.build.entities.len(),
                    state.build.schemas.len(),
                );
                for warn in warnings {
                    result.content.push(Content::text(warn));
                }
                Ok(result)
            }
            Err(e) => Ok(tools::build::error_result(&e.to_string())),
        }
    }

    // -- Write tools (with git commit/push) --

    #[tool(description = "Add a new entity to the workspace. \
        Provide the entity type, ID, and a map of field values (JSON types). \
        The tool validates the entity against the schema, generates the DSL, and writes it to a file. \
        Changes are committed and pushed to the mcp branch for review.")]
    async fn add_entity(
        &self,
        Parameters(params): Parameters<AddEntityParams>,
    ) -> Result<CallToolResult, McpError> {
        debug!("Tool: add_entity, type={}, id={}", params.r#type, params.id);
        let result = {
            let state = self.firm.state().lock().await;
            tools::add_entity::execute(
                self.firm.workspace_path(),
                &state.build,
                &state.graph,
                &params,
            )
        };

        match result {
            Ok(add_result) => {
                let rebuild_err = self.firm.rebuild().await.err();
                let git_warn = self
                    .git_commit_and_push(&format!("Add {}/{} via MCP", params.r#type, params.id))
                    .await;

                let mut tool_result = match rebuild_err {
                    None => tools::add_entity::success_result(add_result),
                    Some(e) => tools::add_entity::warning_result(add_result, &e),
                };
                if let Some(warn) = git_warn {
                    tool_result
                        .content
                        .push(Content::text(warn));
                }
                Ok(tool_result)
            }
            Err(e) => Ok(tools::build::error_result(&e)),
        }
    }

    #[tool(description = "Write DSL content to a .firm source file. \
        The content is validated for correct syntax and semantics (references, schema conformance). \
        If validation fails, changes are rolled back unless 'force' is true. \
        Changes are committed and pushed to the mcp branch for review.")]
    async fn write_source(
        &self,
        Parameters(params): Parameters<WriteSourceParams>,
    ) -> Result<CallToolResult, McpError> {
        debug!(
            "Tool: write_source, path={}, content_len={}, force={}",
            params.path,
            params.content.len(),
            params.force
        );

        let write_result =
            match tools::write_source::validate_and_write(self.firm.workspace_path(), &params) {
                Ok(result) => result,
                Err(error_result) => return Ok(error_result),
            };

        match self.firm.rebuild().await {
            Ok(_) => {
                let mut result = tools::write_source::success_result(
                    &params.path,
                    params.content.len(),
                    write_result.file_existed,
                );
                let git_warn = self
                    .git_commit_and_push(&format!("Update source '{}' via MCP", params.path))
                    .await;
                if let Some(warn) = git_warn {
                    result.content.push(Content::text(warn));
                }
                Ok(result)
            }
            Err(e) => {
                if params.force {
                    let mut result = tools::write_source::force_success_result(
                        &params.path,
                        params.content.len(),
                        write_result.file_existed,
                        &e.to_string(),
                    );
                    let git_warn = self
                        .git_commit_and_push(&format!(
                            "Update source '{}' via MCP (force)",
                            params.path
                        ))
                        .await;
                    if let Some(warn) = git_warn {
                        result.content.push(Content::text(warn));
                    }
                    Ok(result)
                } else {
                    let rollback_success = tools::write_source::rollback(
                        self.firm.workspace_path(),
                        &params.path,
                        write_result.original_content,
                    );
                    Ok(tools::write_source::validation_error_result(
                        &e.to_string(),
                        rollback_success,
                    ))
                }
            }
        }
    }

    #[tool(description = "Replace a specific string in a .firm source file. \
        Validates that old_string exists exactly once (or use replace_all for multiple). \
        The result is validated for correct syntax and semantics. \
        If validation fails, changes are rolled back unless 'force' is true. \
        Changes are committed and pushed to the mcp branch for review.")]
    async fn replace_source(
        &self,
        Parameters(params): Parameters<ReplaceSourceParams>,
    ) -> Result<CallToolResult, McpError> {
        debug!(
            "Tool: replace_source, path={}, replace_all={}, force={}",
            params.path, params.replace_all, params.force
        );

        let replace_result =
            match tools::replace_source::execute(self.firm.workspace_path(), &params) {
                Ok(result) => result,
                Err(error_result) => return Ok(error_result),
            };

        let write_params = WriteSourceParams {
            path: params.path.clone(),
            content: replace_result.new_content.clone(),
            force: params.force,
        };

        let write_result = match tools::write_source::validate_and_write(
            self.firm.workspace_path(),
            &write_params,
        ) {
            Ok(result) => result,
            Err(error_result) => return Ok(error_result),
        };

        match self.firm.rebuild().await {
            Ok(_) => {
                let mut result = tools::replace_source::success_result(
                    &params.path,
                    replace_result.occurrences_replaced,
                );
                let git_warn = self
                    .git_commit_and_push(&format!(
                        "Replace in source '{}' via MCP",
                        params.path
                    ))
                    .await;
                if let Some(warn) = git_warn {
                    result.content.push(Content::text(warn));
                }
                Ok(result)
            }
            Err(e) => {
                if params.force {
                    let mut result = tools::replace_source::force_success_result(
                        &params.path,
                        replace_result.occurrences_replaced,
                        &e.to_string(),
                    );
                    let git_warn = self
                        .git_commit_and_push(&format!(
                            "Replace in source '{}' via MCP (force)",
                            params.path
                        ))
                        .await;
                    if let Some(warn) = git_warn {
                        result.content.push(Content::text(warn));
                    }
                    Ok(result)
                } else {
                    let rollback_success = tools::write_source::rollback(
                        self.firm.workspace_path(),
                        &params.path,
                        write_result.original_content,
                    );
                    Ok(tools::replace_source::validation_error_result(
                        &e.to_string(),
                        rollback_success,
                    ))
                }
            }
        }
    }
}

#[tool_handler]
impl ServerHandler for RemoteFirmServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            protocol_version: ProtocolVersion::LATEST,
            capabilities: ServerCapabilities::builder()
                .enable_resources()
                .enable_tools()
                .build(),
            server_info: Implementation::from_build_env(),
            instructions: Some(
                "Firm Remote MCP server. Changes are committed to the 'mcp' branch \
                 with an open PR for human review. Use 'list schema' to explore available \
                 entity types. Use 'add_entity' to create new entities. Use 'query', 'list', \
                 and 'get' to explore existing data."
                    .into(),
            ),
        }
    }

    async fn list_resources(
        &self,
        _request: Option<PaginatedRequestParam>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListResourcesResult, McpError> {
        let state = self.firm.state().lock().await;

        let mut resource_list: Vec<Resource> = state
            .workspace
            .file_paths()
            .iter()
            .filter_map(|path| {
                resources::to_relative_path(self.firm.workspace_path(), path)
                    .map(|rel| resources::source_file_resource(&rel))
            })
            .collect();

        resource_list.sort_by(|a, b| a.name.cmp(&b.name));

        Ok(ListResourcesResult {
            resources: resource_list,
            next_cursor: None,
            meta: None,
        })
    }

    async fn read_resource(
        &self,
        request: ReadResourceRequestParam,
        _context: RequestContext<RoleServer>,
    ) -> Result<ReadResourceResult, McpError> {
        let uri = &request.uri;

        let relative_path = resources::parse_source_uri(uri).ok_or_else(|| {
            McpError::resource_not_found(format!("Invalid resource URI: {}", uri), None)
        })?;

        let contents = resources::read_source_file(self.firm.workspace_path(), &relative_path)
            .map_err(|e| McpError::resource_not_found(e, None))?;

        Ok(ReadResourceResult {
            contents: vec![ResourceContents::text(contents, uri.clone())],
        })
    }
}
