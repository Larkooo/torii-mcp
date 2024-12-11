use std::sync::Arc;
use async_trait::async_trait;
use serde_json::{json, Value};
use once_cell::sync::Lazy;
use reqwest::Client;

use mcp_rust_sdk::{
    error::Error,
    server::{Server, ServerHandler},
    transport::stdio::StdioTransport,
    types::{ClientCapabilities, Implementation, ServerCapabilities},
};

static CLIENT: Lazy<Client> = Lazy::new(|| Client::new());

struct SqlHandler {
    sql_url: String,
}

#[async_trait]
impl ServerHandler for SqlHandler {
    async fn initialize(
        &self,
        implementation: Implementation,
        _capabilities: ClientCapabilities,
    ) -> Result<ServerCapabilities, Error> {
        println!("Client connected: {} v{}", implementation.name, implementation.version);
        
        Ok(ServerCapabilities {
            custom: None
        })
    }

    async fn handle_method(
        &self,
        method: &str,
        params: Option<Value>,
    ) -> Result<Value, Error> {
        match method {
            "tools/list" => Ok(json!({
                "tools": [
                    {
                        "name": "query",
                        "description": "Execute a SQL query against the remote database",
                        "inputSchema": {
                            "type": "object",
                            "properties": {
                                "query": {
                                    "type": "string",
                                    "description": "SQL query to execute"
                                }
                            },
                            "required": ["query"]
                        }
                    },
                    {
                        "name": "schema",
                        "description": "Retrieve the database schema",
                        "inputSchema": {
                            "type": "object",
                            "properties": {
                                "table": {
                                    "type": "string",
                                    "description": "Optional table name to get schema for. If omitted, returns schema for all tables."
                                }
                            }
                        }
                    }
                ]
            })),
            
            "tools/call" => {
                let params = params.ok_or_else(|| Error::Other("Missing parameters".into()))?;
                let tool_name = params["name"].as_str().ok_or_else(|| Error::Other("Missing tool name".into()))?;
                let arguments = params["arguments"].as_object().ok_or_else(|| Error::Other("Missing arguments".into()))?;

                match tool_name {
                    "query" => {
                        let query = arguments["query"].as_str()
                            .ok_or_else(|| Error::Other("Missing query parameter".into()))?;

                        let response = CLIENT
                            .post(&self.sql_url)
                            .json(&json!({ "query": query }))
                            .send()
                            .await
                            .map_err(|e| Error::Other(e.to_string()))?
                            .error_for_status()
                            .map_err(|e| Error::Other(e.to_string()))?
                            .json::<Value>()
                            .await
                            .map_err(|e| Error::Other(e.to_string()))?;

                        Ok(json!({
                            "content": [{
                                "type": "text",
                                "text": serde_json::to_string_pretty(&response).unwrap()
                            }]
                        }))
                    },
                    "schema" => {
                        let table_filter = arguments.get("table").and_then(|v| v.as_str());
                        
                        let schema_query = match table_filter {
                            Some(table) => format!(
                                "SELECT m.name as table_name, p.* 
                                 FROM sqlite_master m
                                 JOIN pragma_table_info(m.name) p
                                 WHERE m.type = 'table' AND m.name = '{}'
                                 ORDER BY m.name, p.cid", 
                                table
                            ),
                            None => String::from(
                                "SELECT m.name as table_name, p.* 
                                 FROM sqlite_master m
                                 JOIN pragma_table_info(m.name) p
                                 WHERE m.type = 'table'
                                 ORDER BY m.name, p.cid"
                            ),
                        };

                        let response = CLIENT
                            .post(&self.sql_url)
                            .json(&json!({ "query": schema_query }))
                            .send()
                            .await
                            .map_err(|e| Error::Other(e.to_string()))?
                            .error_for_status()
                            .map_err(|e| Error::Other(e.to_string()))?
                            .json::<Vec<Value>>()
                            .await
                            .map_err(|e| Error::Other(e.to_string()))?;

                        let mut schema = serde_json::Map::new();
                        for row in response {
                            let row = row.as_object().unwrap();
                            let table_name = row["table_name"].as_str().unwrap();
                            let column_name = row["name"].as_str().unwrap();
                            
                            let table_entry = schema.entry(table_name.to_string()).or_insert_with(|| {
                                json!({
                                    "columns": serde_json::Map::new()
                                })
                            });

                            if let Some(columns) = table_entry.get_mut("columns").and_then(|v| v.as_object_mut()) {
                                columns.insert(
                                    column_name.to_string(),
                                    json!({
                                        "type": row["type"],
                                        "nullable": row["notnull"] == 0,
                                        "primary_key": row["pk"] == 1,
                                        "default": row["dflt_value"]
                                    }),
                                );
                            }
                        }

                        Ok(json!({
                            "content": [{
                                "type": "text",
                                "text": serde_json::to_string_pretty(&schema).unwrap()
                            }]
                        }))
                    },
                    _ => Err(Error::Other(format!("Unknown tool: {}", tool_name)))
                }
            },

            _ => Err(Error::Other(format!("Unknown method: {}", method)))
        }
    }

    async fn shutdown(&self) -> Result<(), Error> {
        println!("Server shutting down");
        Ok(())
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let sql_url = std::env::args()
        .nth(1)
        .expect("Please provide SQL endpoint URL (e.g., http://localhost:8080/sql)");

    let (transport, _sender) = StdioTransport::new();
    let handler = SqlHandler {
        sql_url,
    };
    
    let server = Server::new(Arc::new(transport), Arc::new(handler));
    server.start().await?;

    Ok(())
}
